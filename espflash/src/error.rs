use crate::flasher::Command;
use csv::Position;
use miette::{Diagnostic, SourceOffset, SourceSpan};
use slip_codec::Error as SlipError;
use std::fmt::{Display, Formatter};
use std::io;
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum Error {
    #[error("Error while connecting to device")]
    #[diagnostic(transparent)]
    Connection(#[source] ConnectionError),
    #[error("Communication error while flashing device")]
    #[diagnostic(transparent)]
    Flashing(#[source] ConnectionError),
    #[error("Supplied elf image is not valid")]
    #[diagnostic(
        code(espflash::invalid_elf),
        help("Try running `cargo clean` and rebuilding the image")
    )]
    InvalidElf(#[from] ElfError),
    #[error("Supplied elf image can not be ran from ram as it includes segments mapped to rom addresses")]
    #[diagnostic(
        code(espflash::not_ram_loadable),
        help("Either build the binary to be all in ram or remove the `--ram` option to load the image to flash")
    )]
    ElfNotRamLoadable,
    #[error("The bootloader returned an error")]
    #[diagnostic(transparent)]
    RomError(#[from] RomError),
    #[error("Chip not recognized, supported chip types are esp8266, esp32 and esp32-c3")]
    #[diagnostic(
        code(espflash::unrecognized_chip),
        help("If your chip is supported, try hard-resetting the device and try again")
    )]
    UnrecognizedChip(#[from] ChipDetectError),
    #[error("Flash chip not supported, flash sizes from 1 to 16MB are supported")]
    #[diagnostic(code(espflash::unrecognized_flash))]
    UnsupportedFlash(#[from] FlashDetectError),
    #[error("Failed to connect to on-device flash")]
    #[diagnostic(code(espflash::flash_connect))]
    FlashConnect,
    #[error(transparent)]
    #[diagnostic(transparent)]
    MalformedPartitionTable(#[from] PartitionTableError),
    #[error("Chip does not support direct boot")]
    UnsupportedDirectBoot,
}

#[derive(Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum ConnectionError {
    #[error("IO error while using serial port: {0}")]
    #[diagnostic(code(espflash::serial_error))]
    Serial(#[source] serial::core::Error),
    #[error("Failed to connect to the device")]
    #[diagnostic(
        code(espflash::connection_failed),
        help("Ensure that the device is connected and the reset and boot pins are not being held down")
    )]
    ConnectionFailed,
    #[error("Serial port not found")]
    #[diagnostic(
        code(espflash::connection_failed),
        help("Ensure that the device is connected and your host recognizes the serial adapter")
    )]
    DeviceNotFound,
    #[error("Timeout while running {0}command")]
    #[diagnostic(code(espflash::timeout))]
    Timeout(TimedOutCommand),
    #[error("Received packet has invalid SLIP framing")]
    #[diagnostic(
        code(espflash::slip_framing),
        help("Try hard-resetting the device and try again, if the error persists your rom might be corrupted")
    )]
    FramingError,
    #[error("Received packet to large for buffer")]
    #[diagnostic(
        code(espflash::oversized_packet),
        help("Try hard-resetting the device and try again, if the error persists your rom might be corrupted")
    )]
    OverSizedPacket,
}

#[derive(Debug, Default, Clone)]
pub struct TimedOutCommand {
    command: Option<Command>,
}

impl From<Command> for TimedOutCommand {
    fn from(c: Command) -> Self {
        TimedOutCommand { command: Some(c) }
    }
}

impl Display for TimedOutCommand {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.command {
            Some(command) => write!(f, "{} ", command),
            None => Ok(()),
        }
    }
}

impl From<serial::Error> for ConnectionError {
    fn from(err: serial::Error) -> Self {
        match err.kind() {
            serial::ErrorKind::Io(kind) => from_error_kind(kind, err),
            serial::ErrorKind::NoDevice => ConnectionError::DeviceNotFound,
            _ => ConnectionError::Serial(err),
        }
    }
}

impl From<serial::Error> for Error {
    fn from(err: serial::Error) -> Self {
        Self::Connection(err.into())
    }
}

impl From<io::Error> for ConnectionError {
    fn from(err: io::Error) -> Self {
        from_error_kind(err.kind(), err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Connection(err.into())
    }
}

fn from_error_kind<E: Into<serial::Error>>(kind: io::ErrorKind, err: E) -> ConnectionError {
    match kind {
        io::ErrorKind::TimedOut => ConnectionError::Timeout(TimedOutCommand::default()),
        io::ErrorKind::NotFound => ConnectionError::DeviceNotFound,
        _ => ConnectionError::Serial(err.into()),
    }
}

impl From<SlipError> for ConnectionError {
    fn from(err: SlipError) -> Self {
        match err {
            SlipError::FramingError => Self::FramingError,
            SlipError::OversizedPacket => Self::OverSizedPacket,
            SlipError::ReadError(io) => Self::from(io),
            SlipError::EndOfStream => Self::FramingError,
        }
    }
}

impl From<SlipError> for Error {
    fn from(err: SlipError) -> Self {
        Self::Connection(err.into())
    }
}

impl From<binread::Error> for ConnectionError {
    fn from(err: binread::Error) -> Self {
        match err {
            binread::Error::Io(e) => ConnectionError::from(e),
            _ => unreachable!(),
        }
    }
}

impl From<binread::Error> for Error {
    fn from(err: binread::Error) -> Self {
        Self::Connection(err.into())
    }
}

#[derive(Copy, Clone, Debug, Error, Diagnostic)]
#[allow(dead_code)]
#[repr(u8)]
#[non_exhaustive]
pub enum RomError {
    #[error("Invalid message received")]
    #[diagnostic(code(espflash::rom::invalid_message))]
    InvalidMessage = 0x05,
    #[error("Bootloader failed to execute command")]
    #[diagnostic(code(espflash::rom::failed))]
    FailedToAct = 0x06,
    #[error("Received message has invalid crc")]
    #[diagnostic(code(espflash::rom::crc))]
    InvalidCrc = 0x07,
    #[error("Bootloader failed to write to flash")]
    #[diagnostic(code(espflash::rom::flash_write))]
    FlashWriteError = 0x08,
    #[error("Bootloader failed to read from flash")]
    #[diagnostic(code(espflash::rom::flash_read))]
    FlashReadError = 0x09,
    #[error("Invalid length for flash read")]
    #[diagnostic(code(espflash::rom::flash_read_length))]
    FlashReadLengthError = 0x0a,
    #[error("Malformed compressed data received")]
    #[diagnostic(code(espflash::rom::deflate))]
    DeflateError = 0x0b,
    #[error("Other")]
    #[diagnostic(code(espflash::rom::other))]
    Other = 0xff,
}

impl From<u8> for RomError {
    fn from(raw: u8) -> Self {
        match raw {
            0x05 => RomError::InvalidMessage,
            0x06 => RomError::FailedToAct,
            0x07 => RomError::InvalidCrc,
            0x08 => RomError::FlashWriteError,
            0x09 => RomError::FlashReadError,
            0x0a => RomError::FlashReadLengthError,
            0x0b => RomError::DeflateError,
            _ => RomError::Other,
        }
    }
}

pub(crate) trait ResultExt {
    /// mark an error as having occurred during the flashing stage
    fn flashing(self) -> Self;
    /// mark the command from which this error originates
    fn for_command(self, command: Command) -> Self;
}

impl<T> ResultExt for Result<T, Error> {
    fn flashing(self) -> Self {
        match self {
            Err(Error::Connection(err)) => Err(Error::Flashing(err)),
            res => res,
        }
    }

    fn for_command(self, command: Command) -> Self {
        match self {
            Err(Error::Connection(ConnectionError::Timeout(_))) => {
                Err(Error::Connection(ConnectionError::Timeout(command.into())))
            }
            Err(Error::Flashing(ConnectionError::Timeout(_))) => {
                Err(Error::Flashing(ConnectionError::Timeout(command.into())))
            }
            res => res,
        }
    }
}

#[derive(Debug, Error, Diagnostic)]
#[error("Malformed partition table")]
#[diagnostic(
    code(espflash::mallformed_partition_table),
    help("See the espressif documentation for information on the partition table format:

https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-guides/partition-tables.html#creating-custom-tables")
)]
pub struct PartitionTableError {
    #[source_code]
    source: String,
    #[label("{}", self.hint)]
    err_span: SourceSpan,
    hint: String,
    #[source]
    error: csv::Error,
}

impl PartitionTableError {
    pub fn new(error: csv::Error, source: String) -> Self {
        let err_pos = match error.kind() {
            csv::ErrorKind::Deserialize { pos: Some(pos), .. } => pos.clone(),
            csv::ErrorKind::UnequalLengths { pos: Some(pos), .. } => pos.clone(),
            _ => Position::new(),
        };
        let hint = match error.kind() {
            csv::ErrorKind::Deserialize { err, .. } => err.to_string(),
            csv::ErrorKind::UnequalLengths {
                expected_len, len, ..
            } => format!(
                "record has {} fields, but the previous record has {} fields",
                len, expected_len
            ),
            _ => String::new(),
        };

        // since csv doesn't give us the position in the line the error occurs, we highlight the entire line
        let line_length = source
            .lines()
            .nth(err_pos.line() as usize - 1)
            .unwrap()
            .len()
            .into();
        let err_span = SourceSpan::new(pos_to_offset(err_pos), line_length);

        PartitionTableError {
            source,
            err_span,
            hint,
            error,
        }
    }
}

fn pos_to_offset(pos: Position) -> SourceOffset {
    (pos.byte() as usize).into()
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ElfError(&'static str);

impl From<&'static str> for ElfError {
    fn from(err: &'static str) -> Self {
        ElfError(err)
    }
}

#[derive(Debug, Error)]
#[error("Unrecognized magic value {0:#x}")]
pub struct ChipDetectError(u32);

impl From<u32> for ChipDetectError {
    fn from(err: u32) -> Self {
        ChipDetectError(err)
    }
}

#[derive(Debug, Error)]
#[error("Unrecognized flash id {0:#x}")]
pub struct FlashDetectError(u8);

impl From<u8> for FlashDetectError {
    fn from(err: u8) -> Self {
        FlashDetectError(err)
    }
}
