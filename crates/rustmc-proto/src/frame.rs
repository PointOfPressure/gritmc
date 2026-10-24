//! Packet framing: length-prefixed frames, optional zlib compression after
//! the server sets a threshold. Encode/decode are pure functions over
//! Read/Write (testable offline); [`Conn`] binds them to a TcpStream.
//!
//! Uncompressed:  varint(len) · varint(id)+body
//! Compressed:    varint(len) · varint(uncompressed_len | 0) · deflate(id+body)
//!                (0 ⇒ id+body follows raw, packet was under threshold)

use crate::varint::{read_varint, write_varint};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Hard cap on a single frame (matches vanilla's 8MiB limit order); guards
/// against a corrupt length prefix allocating the moon.
const MAX_FRAME: i32 = 1 << 23;

pub fn write_frame(
    out: &mut impl Write,
    compression: Option<i32>,
    id: i32,
    body: &[u8],
) -> io::Result<()> {
    let mut packet = Vec::with_capacity(body.len() + 5);
    write_varint(&mut packet, id)?;
    packet.extend_from_slice(body);

    let mut inner = Vec::with_capacity(packet.len() + 6);
    match compression {
        Some(threshold) if packet.len() as i32 >= threshold => {
            write_varint(&mut inner, packet.len() as i32)?;
            let mut enc = ZlibEncoder::new(&mut inner, flate2::Compression::default());
            enc.write_all(&packet)?;
            enc.finish()?;
        }
        Some(_) => {
            write_varint(&mut inner, 0)?;
            inner.extend_from_slice(&packet);
        }
        None => inner = packet,
    }

    write_varint(out, inner.len() as i32)?;
    out.write_all(&inner)
}

pub fn read_frame(r: &mut impl Read, compression: bool) -> io::Result<(i32, Vec<u8>)> {
    let len = read_varint(r)?;
    if !(0..=MAX_FRAME).contains(&len) {
        return Err(io::Error::other(format!("frame length {len} out of range")));
    }
    let mut inner = vec![0u8; len as usize];
    r.read_exact(&mut inner)?;

    let mut cur: &[u8] = &inner;
    let packet: Vec<u8> = if compression {
        let unc_len = read_varint(&mut cur)?;
        if unc_len == 0 {
            cur.to_vec()
        } else {
            if !(0..=MAX_FRAME).contains(&unc_len) {
                return Err(io::Error::other("uncompressed length out of range"));
            }
            let mut out = Vec::with_capacity(unc_len as usize);
            ZlibDecoder::new(cur).read_to_end(&mut out)?;
            if out.len() as i32 != unc_len {
                return Err(io::Error::other("uncompressed length mismatch"));
            }
            out
        }
    } else {
        cur.to_vec()
    };

    let mut pcur: &[u8] = &packet;
    let id = read_varint(&mut pcur)?;
    Ok((id, pcur.to_vec()))
}

/// [`read_frame`] over a reader with a short read timeout, **without losing a
/// frame the timeout lands inside**.
///
/// The server reads each connection with a one-tick (50 ms) timeout so its
/// loop gets back to the clock, and treated every `WouldBlock`/`TimedOut` as
/// "nothing arrived". [`write_frame`] puts the length prefix and the body on
/// the wire in separate writes, so under load the length can arrive a tick
/// before its body: the timeout then fired inside `read_exact`, the bytes
/// already taken were gone, and the next read took the body's middle for a
/// length prefix. The stream was garbage from there on; the read that
/// finally failed did so with `UnexpectedEof`, which the loop took for the
/// client hanging up, closed a socket with unread data, and so reset the
/// client — the `Connection reset by peer` that cost one scenario per
/// structure-mobs subset run (2026-10-04, `docs/FINDINGS.md`).
///
/// The same defect [`Conn::recv_poll`] closed on the recorder's side in
/// August. Here it is generic over the reader so the server's tests can drive
/// it: a timeout before the first byte is returned as is (nothing consumed);
/// once a byte is in, timeouts are waited out until the frame is whole or
/// `patience` passes, which is an `UnexpectedEof`-free `TimedOut` error.
///
/// # Errors
/// The first byte's timeout, a socket error, a malformed frame, or a frame
/// left unfinished for longer than `patience`.
pub fn read_frame_whole(
    r: &mut impl Read,
    compression: bool,
    patience: std::time::Duration,
) -> io::Result<(i32, Vec<u8>)> {
    let mut first = [0u8; 1];
    loop {
        match r.read(&mut first) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let mut rest = Patient {
        first: Some(first[0]),
        inner: r,
        deadline: std::time::Instant::now() + patience,
    };
    read_frame(&mut rest, compression)
}

/// A reader that hands back one consumed byte, then retries the inner
/// reader's timeouts until a deadline.
struct Patient<'a, R: Read> {
    first: Option<u8>,
    inner: &'a mut R,
    deadline: std::time::Instant,
}

impl<R: Read> Read for Patient<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(b) = self.first.take() {
            buf[0] = b;
            return Ok(1);
        }
        loop {
            match self.inner.read(buf) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    if std::time::Instant::now() >= self.deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "a frame was started and never finished",
                        ));
                    }
                }
                other => return other,
            }
        }
    }
}

/// A live connection: framing + compression state over a TcpStream.
pub struct Conn {
    stream: TcpStream,
    /// Compression threshold once the server sends login_compression.
    pub compression: Option<i32>,
}

impl Conn {
    pub fn connect(addr: impl ToSocketAddrs, timeout: Duration) -> io::Result<Conn> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::other("address resolved to nothing"))?;
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        Ok(Conn {
            stream,
            compression: None,
        })
    }

    pub fn set_read_timeout(&mut self, d: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(d)
    }

    pub fn send(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        write_frame(&mut self.stream, self.compression, id, body)?;
        self.stream.flush()
    }

    pub fn recv(&mut self) -> io::Result<(i32, Vec<u8>)> {
        read_frame(&mut self.stream, self.compression.is_some())
    }

    /// Wait up to `poll` for a frame to *start*, then read all of it.
    ///
    /// # The bug this exists to make impossible
    ///
    /// A read timeout is a property of the socket, not of the frame, so a
    /// short one fires wherever it happens to land — including in the middle
    /// of a length-prefixed body. A caller that treats the resulting
    /// `WouldBlock` as "nothing arrived" and loops will read the *middle* of
    /// that frame as the next frame's length prefix, and every packet after
    /// it is garbage. The symptom is a truncated read several frames later,
    /// blaming whichever packet was unlucky.
    ///
    /// It is not hypothetical: it took out every scenario in the gameplay
    /// corpus against rustmc on 2026-08-02 while passing against vanilla,
    /// purely because rustmc's chunk packets are big enough to span the
    /// poll interval and vanilla's, on a flat world, mostly were not. A
    /// harness whose failures depend on how large the server's packets are
    /// is not measuring the server.
    ///
    /// So the short timeout applies to the first byte only. Once any byte of
    /// a frame has been read, the socket is switched to `patience` and the
    /// frame is finished, then the original timeout is restored.
    ///
    /// Returns `Ok(None)` if nothing had arrived — and that answer is only
    /// ever given when **no** bytes were consumed.
    ///
    /// # Errors
    /// On a socket error or a malformed frame.
    pub fn recv_poll(
        &mut self,
        poll: Duration,
        patience: Duration,
    ) -> io::Result<Option<(i32, Vec<u8>)>> {
        self.stream.set_read_timeout(Some(poll))?;
        let mut first = [0u8; 1];
        match self.stream.read_exact(&mut first) {
            Ok(()) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        }
        self.stream.set_read_timeout(Some(patience))?;
        let mut head = Head {
            first: Some(first[0]),
            stream: &mut self.stream,
        };
        let frame = read_frame(&mut head, self.compression.is_some());
        self.stream.set_read_timeout(Some(poll))?;
        frame.map(Some)
    }
}

/// A reader that hands back one already-consumed byte before the socket.
///
/// Needed because the first byte of the length varint has to be read under
/// the short timeout to know whether anything arrived at all, and cannot be
/// pushed back onto a `TcpStream`.
struct Head<'a> {
    first: Option<u8>,
    stream: &'a mut TcpStream,
}

impl Read for Head<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(b) = self.first.take() {
            buf[0] = b;
            return Ok(1);
        }
        self.stream.read(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hands out its chunks one per read and a `WouldBlock` between each,
    /// as a socket with a short timeout does when a frame's length and body
    /// arrive in different ticks.
    struct Ticked {
        chunks: std::collections::VecDeque<Vec<u8>>,
        gap: bool,
    }

    impl Read for Ticked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.gap {
                self.gap = false;
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let Some(mut c) = self.chunks.pop_front() else {
                return Err(io::ErrorKind::WouldBlock.into());
            };
            let n = c.len().min(buf.len());
            buf[..n].copy_from_slice(&c[..n]);
            if n < c.len() {
                self.chunks.push_front(c.split_off(n));
            } else {
                self.gap = true;
            }
            Ok(n)
        }
    }

    /// Two frames, each split at its length prefix with a timeout between:
    /// the patient read returns both whole, in order. The plain read loses
    /// the first body to the timeout, which is the flake's mechanism.
    #[test]
    fn a_timeout_between_a_length_and_its_body_loses_nothing() {
        let mut wire = Vec::new();
        write_frame(&mut wire, None, 0x1d, b"abcdefgh").unwrap();
        write_frame(&mut wire, None, 0x1e, b"xyz").unwrap();
        let split = |w: &[u8]| -> std::collections::VecDeque<Vec<u8>> {
            // [len][body] [len][body]: len is one byte for these sizes.
            let a = 1 + usize::from(w[0]);
            vec![
                w[..1].to_vec(),
                w[1..a].to_vec(),
                w[a..=a].to_vec(),
                w[a + 1..].to_vec(),
            ]
            .into()
        };
        let mut t = Ticked {
            chunks: split(&wire),
            gap: false,
        };
        let patience = std::time::Duration::from_secs(1);
        let a = read_frame_whole(&mut t, false, patience).expect("first");
        assert_eq!(a, (0x1d, b"abcdefgh".to_vec()));
        // The timeout between frames is returned as is: nothing consumed.
        assert_eq!(
            read_frame_whole(&mut t, false, patience).map_err(|e| e.kind()),
            Err(io::ErrorKind::WouldBlock)
        );
        let b = read_frame_whole(&mut t, false, patience).expect("second");
        assert_eq!(b, (0x1e, b"xyz".to_vec()));

        let mut t = Ticked {
            chunks: split(&wire),
            gap: false,
        };
        assert!(
            read_frame(&mut t, false).is_err(),
            "the plain read gives up inside the frame"
        );
        assert_ne!(
            read_frame(&mut t, false).ok(),
            Some((0x1d, b"abcdefgh".to_vec())),
            "and the frame it was inside is gone"
        );
    }

    /// A frame started and never finished is an error after `patience`,
    /// not a hang and not an end of stream.
    #[test]
    fn a_frame_left_unfinished_times_out() {
        let mut t = Ticked {
            chunks: vec![vec![5u8, 0x1d]].into(),
            gap: false,
        };
        let e = read_frame_whole(&mut t, false, std::time::Duration::from_millis(20))
            .expect_err("unfinished");
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    }

    fn roundtrip(compression: Option<i32>, id: i32, body: &[u8]) {
        let mut wire = Vec::new();
        write_frame(&mut wire, compression, id, body).unwrap();
        let (rid, rbody) = read_frame(&mut wire.as_slice(), compression.is_some()).unwrap();
        assert_eq!(rid, id);
        assert_eq!(rbody, body);
    }

    #[test]
    fn uncompressed_roundtrip() {
        roundtrip(None, 0x00, b"");
        roundtrip(None, 0x48, b"hello");
        roundtrip(None, 0x7F, &[0u8; 1000]);
    }

    #[test]
    fn compressed_roundtrip_both_sides_of_threshold() {
        // Below threshold → raw-with-zero-marker path.
        roundtrip(Some(256), 0x02, b"small");
        // Above threshold → actually deflated.
        roundtrip(Some(64), 0x31, &[7u8; 4096]);
        // Threshold boundary exactly.
        roundtrip(Some(5), 0x01, &[1, 2, 3, 4]); // id(1)+4 = 5 ≥ 5 → compressed
    }
}
