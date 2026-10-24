//! `minecraft:component` — `ComponentArgument.textComponent`, for the subset of
//! text components this server can rebuild byte for byte.
//!
//! # What vanilla does
//!
//! `ComponentArgument` is a `ParserBasedArgument` over `TagParser` (SNBT, the
//! packrat grammar) and `ComponentSerialization.CODEC`: the argument is one
//! SNBT value, and that value is decoded as a component. A string tag is a
//! literal; a compound is a component with `text`, the style fields and
//! `extra`. What then reaches a client is the component RE-ENCODED by the
//! network codec, which normalises: a literal with no style and no siblings is
//! a bare `TAG_String`, whatever compound it was typed as.
//!
//! # What this reads, and what it refuses
//!
//! Read: a quoted string (`"..."` or `'...'`, with `\\`, `\"`, `\'`, `\n`, `\t`
//! escapes), an unquoted word that is not a number or a boolean, and a
//! compound whose keys are `text` plus any of `color` (one of the sixteen
//! named colours), `bold`, `italic`, `underlined`, `strikethrough`,
//! `obfuscated` (booleans, `true`/`false` or `0b`/`1b`), `insertion`, and a
//! non-empty `extra` of components of the same kind.
//!
//! Everything else is [`ArgError::Unsupported`] — translatable, score,
//! selector, keybind and NBT contents, click and hover events, hex colours,
//! fonts, a top-level list — so a command carrying one is NOT ANSWERED rather
//! than answered with a component this server would have to guess the
//! encoding of. The accept set is therefore a subset of vanilla's, and every
//! input outside it is refused as unread, never as a syntax error vanilla
//! would not raise.

use crate::cmdreader::Reader;
use crate::commands::{compound, translation_args, Arg};
use crate::commandtree::ArgError;
use rustmc_world::nbt::Nbt;

/// One SNBT value, as far as a text component needs.
#[derive(Debug, Clone, PartialEq)]
enum Snbt {
    Str(String),
    Bool(bool),
    Compound(Vec<(String, Snbt)>),
    List(Vec<Snbt>),
}

/// `ChatFormatting`'s sixteen colours by serialized name.
const NAMED_COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];

/// The style booleans, in the order `Style`'s codec lists them.
const STYLE_FLAGS: [&str; 5] = [
    "bold",
    "italic",
    "underlined",
    "strikethrough",
    "obfuscated",
];

fn unquoted_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
}

fn skip_ws(r: &mut Reader<'_>) {
    while r.peek().is_some_and(char::is_whitespace) {
        r.skip();
    }
}

fn quoted(r: &mut Reader<'_>, q: char) -> Result<String, ArgError> {
    r.skip();
    let mut out = String::new();
    loop {
        match r.read() {
            None => return Err(ArgError::Unsupported),
            Some('\\') => match r.read() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                _ => return Err(ArgError::Unsupported),
            },
            Some(c) if c == q => return Ok(out),
            Some(c) => out.push(c),
        }
    }
}

fn value(r: &mut Reader<'_>, depth: usize) -> Result<Snbt, ArgError> {
    if depth > 16 {
        return Err(ArgError::Unsupported);
    }
    match r.peek() {
        Some(q @ ('"' | '\'')) => quoted(r, q).map(Snbt::Str),
        Some('{') => {
            r.skip();
            let mut fields = Vec::new();
            skip_ws(r);
            if r.peek() == Some('}') {
                r.skip();
                return Ok(Snbt::Compound(fields));
            }
            loop {
                skip_ws(r);
                let key = match r.peek() {
                    Some(q @ ('"' | '\'')) => quoted(r, q)?,
                    _ => {
                        let start = r.cursor();
                        while r.peek().is_some_and(unquoted_char) {
                            r.skip();
                        }
                        if r.cursor() == start {
                            return Err(ArgError::Unsupported);
                        }
                        r.string()[start..r.cursor()].to_string()
                    }
                };
                skip_ws(r);
                if r.read() != Some(':') {
                    return Err(ArgError::Unsupported);
                }
                skip_ws(r);
                let v = value(r, depth + 1)?;
                if fields.iter().any(|(k, _)| *k == key) {
                    return Err(ArgError::Unsupported);
                }
                fields.push((key, v));
                skip_ws(r);
                match r.read() {
                    Some(',') => {}
                    Some('}') => return Ok(Snbt::Compound(fields)),
                    _ => return Err(ArgError::Unsupported),
                }
            }
        }
        Some('[') => {
            r.skip();
            let mut items = Vec::new();
            skip_ws(r);
            if r.peek() == Some(']') {
                r.skip();
                return Ok(Snbt::List(items));
            }
            loop {
                skip_ws(r);
                items.push(value(r, depth + 1)?);
                skip_ws(r);
                match r.read() {
                    Some(',') => {}
                    Some(']') => return Ok(Snbt::List(items)),
                    _ => return Err(ArgError::Unsupported),
                }
            }
        }
        Some(_) => {
            let start = r.cursor();
            while r.peek().is_some_and(unquoted_char) {
                r.skip();
            }
            let word = &r.string()[start..r.cursor()];
            match word {
                "" => Err(ArgError::Unsupported),
                // SNBT reads `true`/`false` case-insensitively.
                w if w.eq_ignore_ascii_case("true") => Ok(Snbt::Bool(true)),
                w if w.eq_ignore_ascii_case("false") => Ok(Snbt::Bool(false)),
                "1b" | "1B" => Ok(Snbt::Bool(true)),
                "0b" | "0B" => Ok(Snbt::Bool(false)),
                // Any other number-shaped word is a numeric tag, and a number
                // is not a component.
                w if w.starts_with(|c: char| {
                    c.is_ascii_digit() || c == '-' || c == '+' || c == '.'
                }) =>
                {
                    Err(ArgError::Unsupported)
                }
                w => Ok(Snbt::Str(w.to_string())),
            }
        }
        None => Err(ArgError::Unsupported),
    }
}

/// The SNBT value as a component in its NETWORK form, or `None` outside the
/// subset this module rebuilds.
fn component(v: &Snbt) -> Option<Nbt> {
    match v {
        Snbt::Str(s) => Some(Nbt::String(s.clone())),
        Snbt::Compound(fields) => {
            let mut text: Option<String> = None;
            let mut style: Vec<(&str, Nbt)> = Vec::new();
            let mut extra: Option<Vec<Nbt>> = None;
            for (k, val) in fields {
                match (k.as_str(), val) {
                    ("text", Snbt::Str(s)) => text = Some(s.clone()),
                    ("type", Snbt::Str(t)) if t == "text" => {}
                    ("color", Snbt::Str(c)) if NAMED_COLORS.contains(&c.as_str()) => {
                        style.push(("color", Nbt::String(c.clone())));
                    }
                    ("insertion", Snbt::Str(s)) => {
                        style.push(("insertion", Nbt::String(s.clone())))
                    }
                    (flag, Snbt::Bool(b)) if STYLE_FLAGS.contains(&flag) => {
                        let name = STYLE_FLAGS.iter().find(|f| **f == flag)?;
                        style.push((name, Nbt::Byte(i8::from(*b))));
                    }
                    ("extra", Snbt::List(items)) if !items.is_empty() => {
                        extra = Some(items.iter().map(component).collect::<Option<Vec<_>>>()?);
                    }
                    _ => return None,
                }
            }
            let text = text?;
            if style.is_empty() && extra.is_none() {
                return Some(Nbt::String(text));
            }
            let mut c = compound([("text", Nbt::String(text))]);
            if let Nbt::Compound(m) = &mut c {
                for (k, v) in style {
                    m.insert(k.to_string(), v);
                }
                if let Some(items) = extra {
                    let args: Vec<Arg> = items
                        .into_iter()
                        .map(|n| match n {
                            Nbt::String(s) => Arg::Text(s),
                            other => Arg::Component(other),
                        })
                        .collect();
                    m.insert("extra".into(), translation_args(&args));
                }
            }
            Some(c)
        }
        Snbt::Bool(_) | Snbt::List(_) => None,
    }
}

/// `ComponentArgument.parse` for the subset — the component, network form.
///
/// # Errors
/// [`ArgError::Unsupported`] for anything outside the subset; see the module
/// header. The reader is put back where it began.
pub fn parse(r: &mut Reader<'_>) -> Result<Nbt, ArgError> {
    let start = r.cursor();
    let out = value(r, 0).and_then(|v| component(&v).ok_or(ArgError::Unsupported));
    if out.is_err() {
        r.set_cursor(start);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str) -> Option<(Nbt, usize)> {
        let mut r = Reader::new(input);
        parse(&mut r).ok().map(|c| (c, r.cursor()))
    }

    #[test]
    fn a_plain_component_is_a_bare_string_whatever_it_was_typed_as() {
        assert_eq!(run("\"hi\""), Some((Nbt::String("hi".into()), 4)));
        assert_eq!(run("{text:\"hi\"}"), Some((Nbt::String("hi".into()), 11)));
        assert_eq!(run("hello world"), Some((Nbt::String("hello".into()), 5)));
    }

    #[test]
    fn a_styled_component_keeps_its_style_as_bytes_and_names() {
        let (c, _) = run("{text:'x', color:red, bold:true, italic:0b}").expect("read");
        let Nbt::Compound(m) = c else { panic!() };
        assert_eq!(m.get("color"), Some(&Nbt::String("red".into())));
        assert_eq!(m.get("bold"), Some(&Nbt::Byte(1)));
        assert_eq!(m.get("italic"), Some(&Nbt::Byte(0)));
    }

    #[test]
    fn outside_the_subset_is_unread_not_wrong() {
        for input in [
            "{translate:\"a\"}",
            "{text:\"a\",color:\"#ff0000\"}",
            "{text:\"a\",clickEvent:{}}",
            "[\"a\"]",
            "42",
            "{text:\"a\",extra:[]}",
        ] {
            let mut r = Reader::new(input);
            assert_eq!(parse(&mut r), Err(ArgError::Unsupported), "{input}");
            assert_eq!(r.cursor(), 0);
        }
    }
}
