use littlefs2::consts::PATH_MAX;
use littlefs2::fs::{Attribute, DirEntry, Filesystem};

use littlefs2::path::{Path, PathBuf};

use postcard;
use serde::{Deserialize, Serialize};

use heapless::Vec;
use heapless_bytes::Bytes;

use trussed::config::{MAX_MESSAGE_LENGTH, USER_ATTRIBUTE_NUMBER};

use trussed::types::{Message, UserAttribute};

pub const MAX_FS_DEPTH: usize = 8;

pub const MAX_DUMP_BLOB_LENGTH: usize = 256 * 10;

const LEN_PREFIX_SIZE: usize = 4;
const FS_BACKUP_START_DELIM: &[u8; 4] = b"SB||";
const FS_BACKUP_END_DELIM: &[u8; 4] = b"||EB";

#[derive(Clone, Debug, PartialEq)]
pub enum FSBackupError {
    LittleFs2Err,
    PathAssemblyErr,
    BackendWriteErr,
    BackendReadErr,
    SerializeErr,
    DeserializeErr,
    PathStackFullErr,
    EndOfBackupBlobs,
    StartOfBackupBlobs,
    RestoreErr,
    UserAttributeErr,
    DataAssemblyErr,
}

impl From<littlefs2::io::Error> for FSBackupError {
    fn from(_error: littlefs2::io::Error) -> Self {
        Self::LittleFs2Err
    }
}

pub type Result<T, E = FSBackupError> = core::result::Result<T, E>;

#[derive(Clone, Debug)]
pub struct PathCursor {
    pub path: PathBuf,
    pub idx: usize,
    pub attr: Option<UserAttribute>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FSEntryBlob {
    path: Bytes<PATH_MAX>,
    is_dir: bool,
    content: Option<Message>,
    attr: Option<UserAttribute>,
}

pub trait BackupBackend {
    // for simplicity we only have one size for read & write
    const RW_SIZE: usize;

    fn read<const N: usize>(&mut self, len: usize) -> Result<Bytes<N>>;
    fn write(&mut self, content: &[u8]) -> Result<usize>;
    fn erase(&mut self) -> Result<usize>;
    fn reset(&mut self);

    fn write_start(&mut self) -> Result<usize> {
        self.write(FS_BACKUP_START_DELIM.as_slice())
    }

    fn write_end(&mut self) -> Result<usize> {
        self.write(FS_BACKUP_END_DELIM.as_slice())
    }

    fn write_entry(
        &mut self,
        entry: &DirEntry,
        content: Option<Message>,
        attr: Option<UserAttribute>,
    ) -> Result<usize> {
        let path_bytes =
            Bytes::<PATH_MAX>::from_slice(entry.path().as_str_ref_with_trailing_nul().as_bytes())
                .map_err(|_| FSBackupError::PathAssemblyErr)?;

        let blob = FSEntryBlob {
            path: path_bytes,
            is_dir: entry.file_type().is_dir(),
            content,
            attr,
        };
        let raw_blob: Vec<u8, MAX_DUMP_BLOB_LENGTH> =
            postcard::to_vec(&blob).map_err(|_| FSBackupError::SerializeErr)?;
        let raw_blob_len: u32 = raw_blob.len() as u32;
        let raw_blob_len_bin = raw_blob_len.to_be_bytes();

        let mut buf = Bytes::<MAX_DUMP_BLOB_LENGTH>::new();
        buf.extend_from_slice(&raw_blob_len_bin)
            .map_err(|_| FSBackupError::DataAssemblyErr)?;
        buf.extend_from_slice(&raw_blob)
            .map_err(|_| FSBackupError::DataAssemblyErr)?;

        self.write(buf.as_slice())
    }

    fn read_next(&mut self) -> Result<FSEntryBlob> {
        let chunk_one: Bytes<MAX_DUMP_BLOB_LENGTH> = self.read(Self::RW_SIZE)?;

        let mut prefix = [0u8; LEN_PREFIX_SIZE];
        prefix.copy_from_slice(&chunk_one.as_slice()[..4]);

        if &prefix == FS_BACKUP_START_DELIM {
            return Err(FSBackupError::StartOfBackupBlobs);
        } else if &prefix == FS_BACKUP_END_DELIM {
            return Err(FSBackupError::EndOfBackupBlobs);
        }
        let blob_len: u32 = u32::from_be_bytes(prefix);

        let cbor_bytes = if blob_len > (Self::RW_SIZE - 4) as u32 {
            let mut buf = Bytes::<MAX_DUMP_BLOB_LENGTH>::new();
            buf.extend_from_slice(&chunk_one.as_slice()[4..])
                .map_err(|_| FSBackupError::DataAssemblyErr)?;
            let remaining_chunks: Bytes<MAX_DUMP_BLOB_LENGTH> =
                self.read(blob_len as usize - (Self::RW_SIZE - 4))?;
            buf.extend_from_slice(&remaining_chunks)
                .map_err(|_| FSBackupError::DataAssemblyErr)?;
            buf
        } else {
            Bytes::from_slice(&chunk_one.as_slice()[4..])
                .map_err(|_| FSBackupError::BackendReadErr)?
        };

        postcard::from_bytes(cbor_bytes.as_slice()).map_err(|_| FSBackupError::DeserializeErr)
    }

    /// return the next filesystem entry inside 'path' after offset 'off'
    fn get_next_entry<S: littlefs2::driver::Storage>(
        fs: &Filesystem<S>,
        path: &PathBuf,
        off: usize,
    ) -> Result<Option<DirEntry>> {
        fs.read_dir_and_then(path, |it| {
            // skip "." & ".."
            Ok(it.enumerate().nth(off + 2))
        })
        .map_err(|_| FSBackupError::LittleFs2Err)
        .map(|v| v.map(|iv| iv.1.unwrap()))
    }

    fn backup<S: littlefs2::driver::Storage>(
        &mut self,
        fs: &Filesystem<S>,
    ) -> Result<(usize, usize)> {
        let root_dir = PathBuf::from("/");

        let mut path_stack: Vec<PathCursor, MAX_FS_DEPTH> = Vec::new();
        path_stack
            .push(PathCursor {
                path: root_dir,
                idx: 0,
                attr: None,
            })
            .map_err(|_| FSBackupError::PathStackFullErr)?;

        self.write_start()?;

        let mut d_cnt: usize = 0;
        let mut f_cnt: usize = 0;
        while !path_stack.is_empty() {
            let mut current = path_stack.pop().unwrap();
            let next_path = Self::get_next_entry(fs, &current.path, current.idx)?;

            // 'None' => no next item inside this directory, continue w/o adding 'current'
            // back to 'path_stack' implicitly means this subtree is done
            if next_path == None {
                continue;
            }

            let entry = next_path.unwrap();

            // move index "pointer" to next item
            current.idx += 1;

            let attr = fs
                .attribute(entry.path(), USER_ATTRIBUTE_NUMBER)?
                .map(|v| UserAttribute::from_slice(v.data()))
                .transpose()
                .map_err(|_| FSBackupError::UserAttributeErr)?;

            if entry.file_type().is_dir() {
                d_cnt += 1;

                let path = PathBuf::from(entry.path());

                path_stack
                    .push(current)
                    .map_err(|_| FSBackupError::PathStackFullErr)?;
                path_stack
                    .push(PathCursor {
                        path,
                        idx: 0,
                        attr: attr.clone(),
                    })
                    .map_err(|_| FSBackupError::PathStackFullErr)?;
                self.write_entry(&entry, None, attr)?;
            } else {
                f_cnt += 1;
                path_stack
                    .push(current)
                    .map_err(|_| FSBackupError::PathStackFullErr)?;

                let file_contents = entry.file_type().is_file().then(|| {
                    Message::from_slice(
                        fs.read::<MAX_MESSAGE_LENGTH>(entry.path())
                            .unwrap()
                            .as_slice(),
                    )
                    .expect("file contents: bytes creation failed")
                });
                self.write_entry(&entry, file_contents, attr)?;
            }
        }

        self.write_end()?;

        Ok((d_cnt, f_cnt))
    }

    fn restore<S: littlefs2::driver::Storage>(
        &mut self,
        fs: &Filesystem<S>,
    ) -> Result<(usize, usize)> {
        let next_entry = self.read_next();
        if let Err(err) = next_entry {
            if err != FSBackupError::StartOfBackupBlobs {
                return Err(err);
            }
        }

        let mut d_cnt: usize = 0;
        let mut f_cnt: usize = 0;
        loop {
            let next_entry = self.read_next();
            match next_entry {
                Ok(v) => {
                    let path = Path::from_bytes_with_nul(v.path.as_slice())
                        .map_err(|_| FSBackupError::PathAssemblyErr)?;
                    if v.is_dir {
                        d_cnt += 1;
                        fs.create_dir(path)?;

                        if let Some(user_attr) = v.attr {
                            let mut attr = Attribute::new(USER_ATTRIBUTE_NUMBER);
                            attr.set_data(user_attr.as_slice());
                            fs.set_attribute(path, &attr)?
                        };
                    } else {
                        f_cnt += 1;
                        let content = v.content.unwrap();
                        fs.write(path, content.as_slice())?;

                        if let Some(user_attr) = v.attr {
                            let mut attr = Attribute::new(USER_ATTRIBUTE_NUMBER);
                            attr.set_data(user_attr.as_slice());
                            fs.set_attribute(path, &attr)?;
                        };
                    }
                }
                Err(e) => {
                    if e == FSBackupError::EndOfBackupBlobs {
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Ok((d_cnt, f_cnt))
    }
}
