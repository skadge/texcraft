//! Git's packet lines: four hexadecimal digits of length (the four included), then the data;
//! `0000` is a flush, `0001` a delimiter. Side-band splits a stream into channels by its
//! packets' first byte: 1 the data, 2 progress messages, 3 an error.

/// The largest packet git sends or takes (65520 bytes, its length included).
pub const MAX_PACKET: usize = 65520;

/// One packet read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packet<'a> {
    Data(&'a [u8]),
    Flush,
    Delimiter,
    /// `0002`, protocol version 2's end of a response.
    ResponseEnd,
}

/// Appends a data packet holding `data`.
pub fn write(out: &mut Vec<u8>, data: &[u8]) {
    // A longer packet is the caller's mistake; split it rather than write a wrong length.
    for chunk in data.chunks(MAX_PACKET - 4) {
        out.extend_from_slice(format!("{:04x}", chunk.len() + 4).as_bytes());
        out.extend_from_slice(chunk);
    }
}

/// Appends a flush packet.
pub fn flush(out: &mut Vec<u8>) {
    out.extend_from_slice(b"0000");
}

/// Reads packets from bytes.
pub struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// What is left after the packets read.
    pub fn rest(&self) -> &'a [u8] {
        self.bytes
    }

    /// The next packet; none at the end; an error when the bytes are not packets.
    pub fn next_packet(&mut self) -> Result<Option<Packet<'a>>, String> {
        if self.bytes.is_empty() {
            return Ok(None);
        }
        let Some(head) = self.bytes.get(..4) else { return Err("a packet line is cut short".to_owned()) };
        let len = std::str::from_utf8(head)
            .ok()
            .and_then(|h| usize::from_str_radix(h, 16).ok())
            .ok_or_else(|| format!("{:?} is not a packet line's length", String::from_utf8_lossy(head)))?;
        let packet = match len {
            0 => Packet::Flush,
            1 => Packet::Delimiter,
            2 => Packet::ResponseEnd,
            3 => return Err("a packet line has a length of 3".to_owned()),
            n if n > MAX_PACKET => return Err(format!("a packet line is longer than git's limit ({n} bytes)")),
            n => Packet::Data(self.bytes.get(4..n).ok_or("a packet line is cut short")?),
        };
        let used = if len < 4 { 4 } else { len };
        self.bytes = &self.bytes[used..];
        Ok(Some(packet))
    }

    /// The next data packet as text without its line feed; none at a flush or the end.
    pub fn next_line(&mut self) -> Result<Option<&'a [u8]>, String> {
        match self.next_packet()? {
            Some(Packet::Data(d)) => Ok(Some(d.strip_suffix(b"\n").unwrap_or(d))),
            _ => Ok(None),
        }
    }
}

/// What a side-band stream carried.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Demuxed {
    /// Channel 1.
    pub data: Vec<u8>,
    /// Channel 2: the server's progress messages, as text.
    pub progress: String,
    /// Channel 3: the server's error, if it sent one.
    pub error: Option<String>,
}

/// Splits side-band packets up to a flush (or the end) into their channels.
pub fn demux(reader: &mut Reader<'_>) -> Result<Demuxed, String> {
    let mut out = Demuxed::default();
    while let Some(packet) = reader.next_packet()? {
        let Packet::Data(data) = packet else { break };
        let Some((&band, rest)) = data.split_first() else { continue };
        match band {
            1 => out.data.extend_from_slice(rest),
            2 => out.progress.push_str(&String::from_utf8_lossy(rest)),
            3 => {
                let text = String::from_utf8_lossy(rest).trim().to_owned();
                out.error = Some(match out.error.take() {
                    Some(e) => format!("{e} {text}"),
                    None => text,
                });
            }
            other => return Err(format!("a side-band packet on channel {other}")),
        }
    }
    Ok(out)
}
