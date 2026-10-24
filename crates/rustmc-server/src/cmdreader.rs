//! `com.mojang.brigadier.StringReader`, transcribed.
//!
//! Brigadier 1.3.10, bundled in the pinned server jar at
//! `META-INF/libraries/com/mojang/brigadier/1.3.10/brigadier-1.3.10.jar`.
//! Every rule below is read from that class file with
//! `javap -p -c -constants`; nothing here is inferred from how a command
//! "obviously" tokenises.
//!
//! # Why this exists as its own module
//!
//! Until 2026-09-05 [`crate::commandtree`] parsed a command by
//! `text.split_whitespace()` and handed each argument node exactly one token.
//! That is a model, not a transcription, and it is wrong in three ways that
//! each block a whole class of command:
//!
//! * **An argument can consume more than one token.** `minecraft:block_pos`
//!   is three coordinates read from one argument node, so `/setblock 1 2 3
//!   stone` has four tokens and two arguments. A one-token-per-node parser
//!   cannot express it at all.
//! * **An argument can consume the whole remainder including spaces.**
//!   `brigadier:string`'s `GREEDY_PHRASE` and `minecraft:message` both take
//!   `getRemaining()`, so `/say hello there` is one argument, not two.
//! * **A token is not a word.** `readString` accepts a quoted phrase with
//!   escapes, and `readUnquotedString` stops at any character outside
//!   `[0-9A-Za-z_.+-]` rather than at a space — so `stone[facing=north]`
//!   ends after `stone` and the rest is the *same* argument's business.
//!
//! # Byte offsets, not char offsets
//!
//! Java's reader indexes UTF-16 code units and this one indexes UTF-8 bytes.
//! That is not a divergence anywhere it can be observed, and the reason is
//! that every character this reader *tests* is ASCII: a byte `>= 0x80` is
//! outside `isAllowedInUnquotedString`, outside `isAllowedNumber`, is neither
//! quote, and is not the space separator, so a multi-byte sequence is never
//! entered and never split. The one place a non-ASCII byte is copied is
//! `read_remaining`, which takes the tail whole.
//!
//! Where the two *would* differ is a cursor arithmetic that crosses a
//! multi-byte character — and [`Reader::skip`] advances by a whole `char`
//! rather than by one byte precisely so that it cannot.

/// Brigadier's `StringReader`.
///
/// `string` and `cursor` are its two fields, with the same names and the same
/// meanings. The cursor is a byte offset into `string` — see the module
/// header for why that is the same reader.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    string: &'a str,
    cursor: usize,
}

/// One Brigadier exception, with everything its message needs.
///
/// # The keys are vanilla's, not Brigadier's
///
/// Brigadier's own `BuiltInExceptions` builds `LiteralMessage`s — English
/// strings. `net.minecraft.commands.BrigadierExceptions` replaces every one of
/// them with a `Component.translatable`, and it is that class's keys that
/// reach a client. They are transcribed beside the reader that raises them
/// because a reader and its message are one fact: a port that raised the right
/// exception with the wrong key would pass every parse test and answer with
/// the wrong sentence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Syntax {
    /// A Brigadier exception whose translation key this server carries.
    Known { key: &'static str, args: SyntaxArgs },
    /// An exception raised by a `net.minecraft.commands.arguments` type, whose
    /// key this server has not transcribed.
    ///
    /// Rendered as nothing rather than as a guess, and counted:
    /// `crates/rustmc-server/tests/command_failure_vanilla.rs` ratchets how
    /// many of the golden's rows land here.
    Unmapped,
}

/// A Brigadier exception's translation arguments.
///
/// `Copy`, deliberately: a [`Syntax`] travels inside a
/// [`crate::commandtree::Refusal`] and through several returns, and an owned
/// `String` in it would make every one of those a clone. The one-argument
/// shape is a **span of the input** rather than the text itself for the same
/// reason — the caller that renders it has the input in hand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyntaxArgs {
    /// No arguments.
    None,
    /// One argument: `input[start..end]`, as a string.
    Span { start: usize, end: usize },
    /// **The BOUND first and the value found second**, which is the opposite
    /// of the order Brigadier passes them in.
    ///
    /// `IntegerArgumentType.parse` raises
    /// `integerTooLow().createWithContext(reader, result, minimum)` — found,
    /// then bound — and `BrigadierExceptions`' `lambda$static$4(a, b)` then
    /// builds `translatable("argument.integer.low", b, a)`, swapping them.
    /// Measured: `/gamerule random_tick_speed -1` produces `with: [0, -1]`.
    Ints(i32, i32),
    /// As [`SyntaxArgs::Ints`], for `brigadier:long`.
    Longs(i64, i64),
    /// As [`SyntaxArgs::Ints`], for `brigadier:float`.
    Floats(f32, f32),
    /// As [`SyntaxArgs::Ints`], for `brigadier:double`.
    Doubles(f64, f64),
}

impl Syntax {
    /// A no-argument exception.
    #[must_use]
    pub const fn simple(key: &'static str) -> Self {
        Self::Known {
            key,
            args: SyntaxArgs::None,
        }
    }

    /// An exception whose one argument is a span of the input.
    #[must_use]
    pub const fn span(key: &'static str, start: usize, end: usize) -> Self {
        Self::Known {
            key,
            args: SyntaxArgs::Span { start, end },
        }
    }
}

impl<'a> Reader<'a> {
    /// `new StringReader(String)`.
    #[must_use]
    pub const fn new(string: &'a str) -> Self {
        Self { string, cursor: 0 }
    }

    /// `getString()`.
    #[must_use]
    pub const fn string(&self) -> &'a str {
        self.string
    }

    /// `getCursor()`.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// `setCursor(int)`.
    ///
    /// Used by every argument type that rewinds on a bounds failure —
    /// `IntegerArgumentType.parse` puts the cursor back before it raises so
    /// that the error is reported at the start of the number rather than
    /// after it.
    ///
    /// # Panics
    /// If `cursor` is not a char boundary, which would make every later read
    /// operate on half a character. A caller can only produce one by
    /// inventing an offset rather than saving one from [`Reader::cursor`].
    pub fn set_cursor(&mut self, cursor: usize) {
        assert!(
            self.string.is_char_boundary(cursor),
            "cursor {cursor} is inside a character of {:?}",
            self.string
        );
        self.cursor = cursor;
    }

    /// `getString().substring(from, to)`.
    ///
    /// `WorldCoordinate.parseDouble` reads the *text* it consumed back out of
    /// the reader — not the value — to decide whether the coordinate was
    /// written with a decimal point, which is what gates the half-block centre
    /// correction. There is no other way to answer that question after the
    /// number has been parsed: `10` and `10.0` are the same `double`.
    ///
    /// # Panics
    /// If either offset is not a char boundary. Both come from
    /// [`Reader::cursor`] at every call site.
    #[must_use]
    pub fn slice(&self, from: usize, to: usize) -> &'a str {
        &self.string[from..to]
    }

    /// `getRemainingLength()`.
    #[must_use]
    pub const fn remaining_len(&self) -> usize {
        self.string.len() - self.cursor
    }

    /// `getRemaining()`.
    #[must_use]
    pub fn remaining(&self) -> &'a str {
        &self.string[self.cursor..]
    }

    /// `canRead(int length)`.
    #[must_use]
    pub const fn can_read_len(&self, length: usize) -> bool {
        self.cursor + length <= self.string.len()
    }

    /// `canRead()`.
    #[must_use]
    pub const fn can_read(&self) -> bool {
        self.can_read_len(1)
    }

    /// `peek()`.
    ///
    /// `None` where Java throws `StringIndexOutOfBoundsException`; every
    /// caller in Brigadier guards with `canRead()` first, and returning an
    /// option rather than panicking keeps a caller that forgets from taking
    /// a connection thread down with it.
    #[must_use]
    pub fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// `read()` — the character at the cursor, advancing past it.
    pub fn read(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.cursor += c.len_utf8();
        Some(c)
    }

    /// `skip()`.
    ///
    /// Advances by a whole character rather than by a byte. In Java this is
    /// `cursor++` over UTF-16 units; here it is the same step for every
    /// character the reader can be positioned on after a test, and a safe
    /// one for the rest. See the module header.
    pub fn skip(&mut self) {
        if let Some(c) = self.peek() {
            self.cursor += c.len_utf8();
        }
    }

    /// `skipWhitespace()`: `while (canRead() && Character.isWhitespace(peek()))
    /// skip()`.
    ///
    /// `Character.isWhitespace` is not Rust's `char::is_whitespace`: Java
    /// excludes the no-break spaces (`U+00A0`, `U+2007`, `U+202F`) and `U+0085`,
    /// and includes the four information separators `U+001C..=U+001F`.
    pub fn skip_whitespace(&mut self) {
        fn java_whitespace(c: char) -> bool {
            matches!(c, '\u{1c}'..='\u{1f}')
                || (c.is_whitespace()
                    && !matches!(c, '\u{85}' | '\u{a0}' | '\u{2007}' | '\u{202f}'))
        }
        while self.peek().is_some_and(java_whitespace) {
            self.skip();
        }
    }

    /// `isAllowedNumber(char)` — `0-9`, `.`, `-`. **Not** `+`, and not `e`:
    /// `1e3` stops after `1` and the `e3` becomes the next node's problem.
    #[must_use]
    pub const fn is_allowed_number(c: char) -> bool {
        c.is_ascii_digit() || c == '.' || c == '-'
    }

    /// `isQuotedStringStart(char)` — a double or single quote.
    #[must_use]
    pub const fn is_quoted_string_start(c: char) -> bool {
        c == '"' || c == '\''
    }

    /// `isAllowedInUnquotedString(char)` — `0-9`, `A-Z`, `a-z`, `_`, `-`,
    /// `.`, `+`, read off the class file's own `bipush` sequence
    /// (95, 45, 46, 43).
    ///
    /// Note what is **not** here: `:` and `/`, which is why a resource
    /// location has a reader of its own rather than being an unquoted
    /// string.
    #[must_use]
    pub const fn is_allowed_in_unquoted_string(c: char) -> bool {
        c.is_ascii_digit()
            || c.is_ascii_uppercase()
            || c.is_ascii_lowercase()
            || c == '_'
            || c == '-'
            || c == '.'
            || c == '+'
    }

    /// `readUnquotedString()`. Never fails: an empty run is an empty string.
    pub fn read_unquoted_string(&mut self) -> &'a str {
        let start = self.cursor;
        while self.peek().is_some_and(Self::is_allowed_in_unquoted_string) {
            self.skip();
        }
        &self.string[start..self.cursor]
    }

    /// `readStringUntil(char)` — the body of a quoted string, with `\` as the
    /// escape and only the terminator and `\` itself escapable.
    ///
    /// Returns an owned `String` because an escape makes the value differ
    /// from any slice of the input.
    ///
    /// # Errors
    /// `parsing.quote.escape` for `\` before anything else,
    /// `parsing.quote.expected.end` if the input runs out first.
    pub fn read_string_until(&mut self, terminator: char) -> Result<String, Syntax> {
        let mut out = String::new();
        let mut escaped = false;
        while let Some(c) = self.read() {
            if escaped {
                if c == terminator || c == '\\' {
                    out.push(c);
                    escaped = false;
                } else {
                    // Java's `setCursor(getCursor() - 1)`: the error points
                    // at the offending character, not past it.
                    self.cursor -= c.len_utf8();
                    // `readerInvalidEscape(String.valueOf(c))` — the one
                    // offending character, which the cursor now points at.
                    return Err(Syntax::span(
                        "parsing.quote.escape",
                        self.cursor,
                        self.cursor + c.len_utf8(),
                    ));
                }
            } else if c == '\\' {
                escaped = true;
            } else if c == terminator {
                return Ok(out);
            } else {
                out.push(c);
            }
        }
        Err(Syntax::simple("parsing.quote.expected.end"))
    }

    /// `readQuotedString()`.
    ///
    /// **An empty reader is an empty string, not an error** — the `canRead()`
    /// guard returns `""` before the quote check runs.
    ///
    /// # Errors
    /// `parsing.quote.expected.start` if the next character is not a quote,
    /// plus whatever [`Reader::read_string_until`] raises.
    pub fn read_quoted_string(&mut self) -> Result<String, Syntax> {
        let Some(next) = self.peek() else {
            return Ok(String::new());
        };
        if !Self::is_quoted_string_start(next) {
            return Err(Syntax::simple("parsing.quote.expected.start"));
        }
        self.skip();
        self.read_string_until(next)
    }

    /// `readString()` — quoted if it starts with a quote, unquoted otherwise.
    ///
    /// # Errors
    /// Only from the quoted branch.
    pub fn read_string(&mut self) -> Result<String, Syntax> {
        match self.peek() {
            None => Ok(String::new()),
            Some(next) if Self::is_quoted_string_start(next) => {
                self.skip();
                self.read_string_until(next)
            }
            Some(_) => Ok(self.read_unquoted_string().to_owned()),
        }
    }

    /// The character run every numeric read shares: `isAllowedNumber` until
    /// it stops. Not a Brigadier method — the four readers each inline it —
    /// but factoring it here keeps the four from drifting apart.
    fn read_number_run(&mut self) -> &'a str {
        let start = self.cursor;
        while self.peek().is_some_and(Self::is_allowed_number) {
            self.skip();
        }
        &self.string[start..self.cursor]
    }

    /// `readInt()`.
    ///
    /// On an invalid number Java rewinds to the start of the run before
    /// raising, and so does this.
    ///
    /// # Errors
    /// `parsing.int.expected` on an empty run, `parsing.int.invalid` on
    /// anything `Integer.parseInt` refuses — which includes overflow.
    pub fn read_int(&mut self) -> Result<i32, Syntax> {
        let start = self.cursor;
        let run = self.read_number_run();
        if run.is_empty() {
            return Err(Syntax::simple("parsing.int.expected"));
        }
        let end = self.cursor;
        run.parse::<i32>().map_err(|_| {
            // Java rewinds BEFORE raising, and the message carries the run it
            // could not parse — so the span is captured first and the cursor
            // put back second.
            self.cursor = start;
            Syntax::span("parsing.int.invalid", start, end)
        })
    }

    /// `readLong()`.
    ///
    /// # Errors
    /// As [`Reader::read_int`].
    pub fn read_long(&mut self) -> Result<i64, Syntax> {
        let start = self.cursor;
        let run = self.read_number_run();
        if run.is_empty() {
            return Err(Syntax::simple("parsing.long.expected"));
        }
        let end = self.cursor;
        run.parse::<i64>().map_err(|_| {
            // Java rewinds BEFORE raising, and the message carries the run it
            // could not parse — so the span is captured first and the cursor
            // put back second.
            self.cursor = start;
            Syntax::span("parsing.long.invalid", start, end)
        })
    }

    /// `readDouble()`.
    ///
    /// # Errors
    /// As [`Reader::read_int`].
    pub fn read_double(&mut self) -> Result<f64, Syntax> {
        let start = self.cursor;
        let run = self.read_number_run();
        if run.is_empty() {
            return Err(Syntax::simple("parsing.double.expected"));
        }
        let end = self.cursor;
        run.parse::<f64>().map_err(|_| {
            // Java rewinds BEFORE raising, and the message carries the run it
            // could not parse — so the span is captured first and the cursor
            // put back second.
            self.cursor = start;
            Syntax::span("parsing.double.invalid", start, end)
        })
    }

    /// `readFloat()`.
    ///
    /// # Errors
    /// As [`Reader::read_int`].
    pub fn read_float(&mut self) -> Result<f32, Syntax> {
        let start = self.cursor;
        let run = self.read_number_run();
        if run.is_empty() {
            return Err(Syntax::simple("parsing.float.expected"));
        }
        let end = self.cursor;
        run.parse::<f32>().map_err(|_| {
            // Java rewinds BEFORE raising, and the message carries the run it
            // could not parse — so the span is captured first and the cursor
            // put back second.
            self.cursor = start;
            Syntax::span("parsing.float.invalid", start, end)
        })
    }

    /// `readBoolean()`.
    ///
    /// **It goes through `readString()`, not `readUnquotedString()`**, so
    /// `/gamerule keep_inventory "true"` is accepted by vanilla and the
    /// single-token tokeniser this replaced refused it.
    ///
    /// # Errors
    /// `parsing.bool.expected` on an empty read, `parsing.bool.invalid` on
    /// anything but `true` or `false`.
    pub fn read_boolean(&mut self) -> Result<bool, Syntax> {
        let start = self.cursor;
        let end_of_value = self.cursor;
        let value = self.read_string()?;
        if value.is_empty() {
            return Err(Syntax::simple("parsing.bool.expected"));
        }
        match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => {
                let end = self.cursor;
                self.cursor = start;
                let _ = end_of_value;
                Err(Syntax::span("parsing.bool.invalid", start, end))
            }
        }
    }

    /// Consume the whole remainder, as `StringArgumentType`'s greedy branch
    /// and `MessageArgument` both do: `getRemaining()` then
    /// `setCursor(getTotalLength())`.
    pub fn read_remaining(&mut self) -> &'a str {
        let rest = self.remaining();
        self.cursor = self.string.len();
        rest
    }

    /// `Identifier.isAllowedInIdentifier(char)` — `0-9`, `a-z`, `_`, `:`,
    /// `/`, `.`, `-`.
    ///
    /// **A different set from [`Reader::is_allowed_in_unquoted_string`] in
    /// both directions**, which is why an identifier has a reader of its own:
    /// it adds `:` and `/`, and it drops `A-Z` and `+`. `foo:BAR` therefore
    /// reads as `foo:` and leaves `BAR`, and `Foo` reads as nothing at all.
    #[must_use]
    pub const fn is_allowed_in_identifier(c: char) -> bool {
        c.is_ascii_digit()
            || c.is_ascii_lowercase()
            || c == '_'
            || c == ':'
            || c == '/'
            || c == '.'
            || c == '-'
    }

    /// `Identifier.readGreedy` — the identifier-character run, which like
    /// [`Reader::read_unquoted_string`] never fails and may be empty.
    pub fn read_identifier_run(&mut self) -> &'a str {
        let start = self.cursor;
        while self.peek().is_some_and(Self::is_allowed_in_identifier) {
            self.skip();
        }
        &self.string[start..self.cursor]
    }

    /// `expect(char)` — the character or a refusal, without consuming on
    /// failure.
    ///
    /// # Errors
    /// `parsing.expected` if the next character is not `c`.
    pub fn expect(&mut self, c: char) -> Result<(), Syntax> {
        if self.peek() == Some(c) {
            self.skip();
            Ok(())
        } else {
            Err(Syntax::simple("parsing.expected"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unquoted set, at both ends of every range and on all four of the
    /// individually-named characters, with the two that are conspicuously
    /// **out** of it pinned: `:` and `/` are what a resource location is
    /// made of, and a reader that allowed them would swallow one whole.
    #[test]
    fn the_unquoted_character_set_is_the_bipush_sequence() {
        for c in ['0', '9', 'A', 'Z', 'a', 'z', '_', '-', '.', '+'] {
            assert!(
                Reader::is_allowed_in_unquoted_string(c),
                "{c:?} must be allowed"
            );
        }
        for c in [':', '/', ' ', '"', '\'', '[', ']', '{', '}', '@', '~', '^'] {
            assert!(
                !Reader::is_allowed_in_unquoted_string(c),
                "{c:?} must not be allowed"
            );
        }
    }

    /// `isAllowedNumber` has no `+` and no `e`, which is the difference
    /// between "stops after the number" and "eats the exponent".
    #[test]
    fn the_number_set_is_digits_dot_and_minus_only() {
        for c in ['0', '9', '.', '-'] {
            assert!(Reader::is_allowed_number(c), "{c:?} must be allowed");
        }
        for c in ['+', 'e', 'E', 'f', 'd', ' '] {
            assert!(!Reader::is_allowed_number(c), "{c:?} must not be allowed");
        }
    }

    #[test]
    fn an_unquoted_string_stops_at_the_first_disallowed_character() {
        let mut r = Reader::new("stone[facing=north] 3");
        assert_eq!(r.read_unquoted_string(), "stone");
        assert_eq!(r.cursor(), 5);
        assert_eq!(r.remaining(), "[facing=north] 3");
    }

    #[test]
    fn a_quoted_string_takes_its_escapes() {
        let mut r = Reader::new(r#""a\"b\\c" tail"#);
        assert_eq!(r.read_string().expect("quoted"), r#"a"b\c"#);
        assert_eq!(r.remaining(), " tail");
    }

    /// A `\` before anything but the terminator or `\` is refused, and the
    /// cursor is left **on** the offending character rather than past it.
    #[test]
    fn an_invalid_escape_is_refused_at_the_offending_character() {
        let mut r = Reader::new(r#""a\nb""#);
        assert_eq!(
            r.read_string(),
            Err(Syntax::span("parsing.quote.escape", 3, 4))
        );
        assert_eq!(r.peek(), Some('n'));
    }

    #[test]
    fn an_unterminated_quote_is_refused() {
        let mut r = Reader::new("\"abc");
        assert_eq!(
            r.read_string(),
            Err(Syntax::simple("parsing.quote.expected.end"))
        );
    }

    /// The bounds-free readers, including the two ways Java says no.
    #[test]
    fn the_numeric_readers_are_javas() {
        assert_eq!(Reader::new("42").read_int(), Ok(42));
        assert_eq!(Reader::new("-42").read_int(), Ok(-42));
        let expected_int = Err(Syntax::simple("parsing.int.expected"));
        assert_eq!(Reader::new("").read_int(), expected_int);
        assert_eq!(Reader::new("x").read_int(), expected_int);
        assert_eq!(
            Reader::new("1.5").read_int(),
            Err(Syntax::span("parsing.int.invalid", 0, 3))
        );
        // `Integer.parseInt` overflows on this and so does `i32::from_str`.
        assert_eq!(
            Reader::new("2147483648").read_int(),
            Err(Syntax::span("parsing.int.invalid", 0, 10))
        );
        assert_eq!(Reader::new("2147483647").read_int(), Ok(i32::MAX));
        assert_eq!(Reader::new("1.5").read_double(), Ok(1.5));
        assert_eq!(Reader::new("-1.5").read_float(), Ok(-1.5));
        assert_eq!(
            Reader::new("1.2.3").read_double(),
            Err(Syntax::span("parsing.double.invalid", 0, 5))
        );
    }

    /// An invalid number leaves the cursor where the number started, so the
    /// next candidate node sees the same input this one did.
    #[test]
    fn an_invalid_number_rewinds() {
        let mut r = Reader::new("hello 1.2.3");
        r.set_cursor(6);
        assert_eq!(
            r.read_double(),
            Err(Syntax::span("parsing.double.invalid", 6, 11))
        );
        assert_eq!(r.cursor(), 6);
    }

    /// `readBoolean` goes through `readString`, so a quoted `true` is a
    /// `true`. The single-token tokeniser this module replaced refused it.
    #[test]
    fn a_quoted_boolean_is_a_boolean() {
        assert_eq!(Reader::new("true").read_boolean(), Ok(true));
        assert_eq!(Reader::new("\"true\"").read_boolean(), Ok(true));
        assert_eq!(Reader::new("'false'").read_boolean(), Ok(false));
        assert_eq!(
            Reader::new("yes").read_boolean(),
            Err(Syntax::span("parsing.bool.invalid", 0, 3))
        );
        assert_eq!(
            Reader::new("").read_boolean(),
            Err(Syntax::simple("parsing.bool.expected"))
        );
    }

    /// A non-ASCII character is outside every character class this reader
    /// tests, so it terminates an unquoted run and survives a greedy one
    /// whole. This is the property that makes byte offsets safe; see the
    /// module header.
    #[test]
    fn a_multibyte_character_is_never_entered_or_split() {
        let mut r = Reader::new("abcé def");
        assert_eq!(r.read_unquoted_string(), "abc");
        assert_eq!(r.cursor(), 3);
        assert_eq!(r.read_remaining(), "é def");
        let mut r = Reader::new("é");
        r.skip();
        assert_eq!(r.cursor(), 2, "skip steps a whole character, not a byte");
        assert!(!r.can_read());
    }

    /// `readQuotedString` on an exhausted reader is an empty string, not a
    /// refusal — the `canRead()` guard is above the quote check.
    #[test]
    fn an_exhausted_reader_quotes_to_the_empty_string() {
        assert_eq!(Reader::new("").read_quoted_string(), Ok(String::new()));
        assert_eq!(
            Reader::new("x").read_quoted_string(),
            Err(Syntax::simple("parsing.quote.expected.start"))
        );
    }
}
