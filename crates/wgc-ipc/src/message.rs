//! One pipe message: a 6-byte header, then a cereal
//! `std::vector<std::unique_ptr<Ipc::Command>>`.
//!
//! Each element starts with a polymorphic type id. The first use of a type in
//! a message sets the high bit and carries the type name and the class
//! version; later uses repeat only the id. The `Ipc::Command` base class
//! version follows the derived one on the first element only.

use zeroize::Zeroizing;

use crate::command::Command;
use crate::command::CommandKind;
use crate::error::CodecError;
use crate::wire::Reader;
use crate::wire::Writer;

pub const MAGIC: u16 = 0xDCEA;
pub const PROTOCOL_VERSION: u32 = 200;

const NEW_TYPE_BIT: u32 = 0x8000_0000;

/// A cereal class version. It decides which optional fields are present, so
/// responses repeat the request's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassVersion(pub u32);

impl ClassVersion {
    pub(crate) fn read_since<T, F>(self, since: u32, read: F) -> Result<Option<T>, CodecError>
    where
        F: FnOnce() -> Result<T, CodecError>,
    {
        if self.0 < since {
            return Ok(None);
        }

        read().map(Some)
    }

    pub(crate) fn write_since<T, F>(
        self,
        since: u32,
        (command, field): (&'static str, &'static str),
        value: Option<T>,
        write: F,
    ) -> Result<(), CodecError>
    where
        F: FnOnce(T),
    {
        match (self.0 >= since, value) {
            (true, Some(value)) => write(value),
            (false, None) => {}
            _ => return Err(CodecError::VersionedField { command, field, version: self.0 }),
        }

        Ok(())
    }
}

/// Responses are matched to requests by this value, 32 hex digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandUid(pub String);

/// The `Ipc::Command` flag byte, which the client sets for asynchronous
/// commands. Responses repeat it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandFlags(pub u8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFrame {
    pub class_version: ClassVersion,
    pub base_class_version: ClassVersion,
    pub uid: CommandUid,
    pub flags: CommandFlags,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub commands: Vec<CommandFrame>,
}

impl Message {
    pub fn decode(bytes: &[u8]) -> Result<Message, CodecError> {
        let mut reader = Reader::new(bytes);

        let magic = reader.u16()?;
        if magic != MAGIC {
            return Err(CodecError::BadMagic { found: magic, expected: MAGIC });
        }
        let protocol = reader.u32()?;
        if protocol != PROTOCOL_VERSION {
            return Err(CodecError::UnsupportedProtocol { found: protocol, expected: PROTOCOL_VERSION });
        }

        let count = reader.u64()?;
        let mut registered: Vec<(CommandKind, ClassVersion)> = Vec::new();
        let mut base_class_version = None;
        let mut commands = Vec::new();
        for index in 0..count {
            let id = reader.u32()?;
            if id == 0 {
                return Err(CodecError::NullCommand { index });
            }

            let first_use = id & NEW_TYPE_BIT != 0;
            let kind = if first_use {
                let name = reader.string()?;
                CommandKind::from_type_name(&name).ok_or(CodecError::UnknownCommand { name })?
            } else {
                registered.get(id as usize - 1).ok_or(CodecError::UnregisteredType { index, id })?.0
            };

            if reader.u8()? == 0 {
                return Err(CodecError::NullCommand { index });
            }

            let class_version = if first_use {
                let version = ClassVersion(reader.u32()?);
                registered.push((kind, version));
                version
            } else {
                registered[id as usize - 1].1
            };

            let base_class_version = match base_class_version {
                Some(version) => version,
                None => *base_class_version.insert(ClassVersion(reader.u32()?)),
            };

            let uid = CommandUid(reader.wstring()?);
            let flags = CommandFlags(reader.u8()?);
            let command = Command::decode(kind, class_version, &mut reader)?;
            commands.push(CommandFrame { class_version, base_class_version, uid, flags, command });
        }

        if reader.remaining() != 0 {
            return Err(CodecError::TrailingBytes { remaining: reader.remaining() });
        }

        Ok(Message { commands })
    }

    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>, CodecError> {
        let mut writer = Writer::new();
        writer.u16(MAGIC);
        writer.u32(PROTOCOL_VERSION);
        writer.u64(self.commands.len() as u64);

        let mut registered: Vec<(CommandKind, ClassVersion)> = Vec::new();
        let mut base_class_version: Option<ClassVersion> = None;
        for frame in &self.commands {
            let kind = frame.command.kind();
            match registered.iter().position(|(registered_kind, _)| *registered_kind == kind) {
                Some(position) => {
                    let first = registered[position].1;
                    if first != frame.class_version {
                        return Err(CodecError::ConflictingClassVersion {
                            command: kind.short_name(),
                            first: first.0,
                            second: frame.class_version.0,
                        });
                    }
                    writer.u32(position as u32 + 1);
                    writer.u8(1);
                }
                None => {
                    registered.push((kind, frame.class_version));
                    writer.u32(NEW_TYPE_BIT | registered.len() as u32);
                    writer.string(&kind.type_name());
                    writer.u8(1);
                    writer.u32(frame.class_version.0);
                }
            }

            match base_class_version {
                Some(first) if first != frame.base_class_version => {
                    return Err(CodecError::ConflictingClassVersion {
                        command: "Command",
                        first: first.0,
                        second: frame.base_class_version.0,
                    });
                }
                Some(_) => {}
                None => {
                    base_class_version = Some(frame.base_class_version);
                    writer.u32(frame.base_class_version.0);
                }
            }

            writer.wstring(&frame.uid.0);
            writer.u8(frame.flags.0);
            frame.command.encode(frame.class_version, &mut writer)?;
        }

        Ok(writer.into_bytes())
    }
}
