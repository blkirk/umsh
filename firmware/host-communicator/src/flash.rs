//! The Pager's chat partition, as a file: a flash in memory whose every
//! write and erase also reaches the file, so the store finds its chats
//! there when the simulator starts again.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::Path;

use embedded_storage_async::nor_flash::{
    ErrorType, MultiwriteNorFlash, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use umsh_communicator::{RamFlash, RamFlashError};

/// The chat partition's size on the Pager.
pub const SIZE: usize = 2 * 1024 * 1024;

pub struct FileFlash {
    flash: RamFlash,
    file: File,
}

#[derive(Debug)]
pub enum FileFlashError {
    Flash(RamFlashError),
    File,
}

impl NorFlashError for FileFlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::Flash(error) => error.kind(),
            Self::File => NorFlashErrorKind::Other,
        }
    }
}

impl FileFlash {
    /// The flash kept in `path`, made erased if there is none yet.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes.len() != SIZE {
            bytes = vec![0xFF; SIZE];
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&bytes)?;
        }
        Ok(Self {
            flash: RamFlash::from_bytes(bytes),
            file,
        })
    }

    /// The octets in `range` reach the file as the flash now holds them.
    fn keep(&mut self, range: Range<usize>) -> Result<(), FileFlashError> {
        let kept = self
            .file
            .seek(SeekFrom::Start(range.start as u64))
            .and_then(|_| self.file.write_all(&self.flash.bytes()[range]));
        kept.map_err(|_| FileFlashError::File)
    }
}

impl ErrorType for FileFlash {
    type Error = FileFlashError;
}

impl ReadNorFlash for FileFlash {
    const READ_SIZE: usize = RamFlash::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.flash
            .read(offset, bytes)
            .await
            .map_err(FileFlashError::Flash)
    }

    fn capacity(&self) -> usize {
        SIZE
    }
}

impl NorFlash for FileFlash {
    const WRITE_SIZE: usize = RamFlash::WRITE_SIZE;
    const ERASE_SIZE: usize = RamFlash::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.flash
            .erase(from, to)
            .await
            .map_err(FileFlashError::Flash)?;
        self.keep(from as usize..to as usize)
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.flash
            .write(offset, bytes)
            .await
            .map_err(FileFlashError::Flash)?;
        self.keep(offset as usize..offset as usize + bytes.len())
    }
}

impl MultiwriteNorFlash for FileFlash {}
