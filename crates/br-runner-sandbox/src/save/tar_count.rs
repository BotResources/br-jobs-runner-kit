const BLOCK: usize = 512;
const META_LIMIT: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TarError {
    #[error("a tar header has a wrong checksum")]
    Checksum,
    #[error("a tar header has a size that is not valid")]
    Size,
    #[error("a pax extended header is larger than {META_LIMIT} bytes")]
    MetaTooLarge,
    #[error("the tar stream ends before its end-of-archive blocks")]
    Truncated,
}

#[derive(Debug)]
enum State {
    Header(Vec<u8>),
    Meta {
        data: Vec<u8>,
        size: u64,
        padding: u64,
    },
    Skip(u64),
    End,
}

#[derive(Debug)]
pub struct TarCounter {
    state: State,
    entries: u64,
    size_override: Option<u64>,
    lone_zero_block: bool,
}

impl Default for TarCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TarCounter {
    pub fn new() -> Self {
        Self {
            state: State::Header(Vec::with_capacity(BLOCK)),
            entries: 0,
            size_override: None,
            lone_zero_block: false,
        }
    }

    pub fn entries(&self) -> u64 {
        self.entries
    }

    pub fn push(&mut self, mut bytes: &[u8]) -> Result<(), TarError> {
        while !bytes.is_empty() {
            bytes = self.step(bytes)?;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<u64, TarError> {
        match self.state {
            State::End => Ok(self.entries),
            State::Header(_) | State::Meta { .. } | State::Skip(_) => Err(TarError::Truncated),
        }
    }

    fn step<'a>(&mut self, bytes: &'a [u8]) -> Result<&'a [u8], TarError> {
        match &mut self.state {
            State::End => Ok(&[]),
            State::Skip(remaining) => {
                let taken = usize::try_from(*remaining)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                *remaining -= taken as u64;
                if *remaining == 0 {
                    self.state = State::Header(Vec::with_capacity(BLOCK));
                }
                Ok(&bytes[taken..])
            }
            State::Meta {
                data,
                size,
                padding,
            } => {
                let missing = usize::try_from(*size + *padding - data.len() as u64)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                data.extend_from_slice(&bytes[..missing]);
                if data.len() as u64 == *size + *padding {
                    data.truncate(usize::try_from(*size).unwrap_or(0));
                    if let Some(size) = pax_size(data) {
                        self.size_override = Some(size);
                    }
                    self.state = State::Header(Vec::with_capacity(BLOCK));
                }
                Ok(&bytes[missing..])
            }
            State::Header(buffer) => {
                let missing = (BLOCK - buffer.len()).min(bytes.len());
                buffer.extend_from_slice(&bytes[..missing]);
                if buffer.len() == BLOCK {
                    let block = std::mem::take(buffer);
                    self.header(&block)?;
                }
                Ok(&bytes[missing..])
            }
        }
    }

    fn header(&mut self, block: &[u8]) -> Result<(), TarError> {
        if block.iter().all(|byte| *byte == 0) {
            self.state = if self.lone_zero_block {
                State::End
            } else {
                self.lone_zero_block = true;
                State::Header(Vec::with_capacity(BLOCK))
            };
            return Ok(());
        }
        self.lone_zero_block = false;
        if !checksum_matches(block) {
            return Err(TarError::Checksum);
        }
        let size = header_size(block)?;
        let kind = block[156];
        match kind {
            b'x' => {
                if size > META_LIMIT {
                    return Err(TarError::MetaTooLarge);
                }
                self.state = State::Meta {
                    data: Vec::new(),
                    size,
                    padding: padding(size),
                };
            }
            b'g' | b'L' | b'K' => self.skip(size),
            _ => {
                self.entries += 1;
                let declared = self.size_override.take().unwrap_or(size);
                let data = if matches!(kind, b'2' | b'3' | b'4' | b'5' | b'6') {
                    0
                } else {
                    declared
                };
                self.skip(data);
            }
        }
        Ok(())
    }

    fn skip(&mut self, size: u64) {
        let total = size.saturating_add(padding(size));
        self.state = if total == 0 {
            State::Header(Vec::with_capacity(BLOCK))
        } else {
            State::Skip(total)
        };
    }
}

fn padding(size: u64) -> u64 {
    let rest = size % BLOCK as u64;
    if rest == 0 { 0 } else { BLOCK as u64 - rest }
}

fn checksum_matches(block: &[u8]) -> bool {
    let stored = octal(&block[148..156]);
    let sum: u64 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    stored == Some(sum)
}

fn octal(field: &[u8]) -> Option<u64> {
    let digits: Vec<u8> = field
        .iter()
        .copied()
        .skip_while(|byte| *byte == b' ')
        .take_while(|byte| !matches!(*byte, 0 | b' '))
        .collect();
    if digits.is_empty() {
        return Some(0);
    }
    digits.iter().try_fold(0u64, |value, byte| {
        if (b'0'..=b'7').contains(byte) {
            value.checked_mul(8)?.checked_add(u64::from(byte - b'0'))
        } else {
            None
        }
    })
}

fn header_size(block: &[u8]) -> Result<u64, TarError> {
    let field = &block[124..136];
    if field[0] & 0x80 != 0 {
        if field[0] != 0x80 || field[1..4].iter().any(|byte| *byte != 0) {
            return Err(TarError::Size);
        }
        return Ok(field[4..]
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)));
    }
    octal(field).ok_or(TarError::Size)
}

fn pax_size(data: &[u8]) -> Option<u64> {
    let mut rest = data;
    let mut found = None;
    while !rest.is_empty() {
        let space = rest.iter().position(|byte| *byte == b' ')?;
        let length: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        if length <= space + 1 || length > rest.len() {
            return None;
        }
        let record = &rest[space + 1..length];
        let record = record.strip_suffix(b"\n")?;
        if let Some(value) = record.strip_prefix(b"size=") {
            found = std::str::from_utf8(value).ok()?.parse().ok();
        }
        rest = &rest[length..];
    }
    found
}
