//! `.device` units are, in the common case, synthesized at runtime by the netlink
//! uevent listener (see `crate::device_events`) rather than loaded from a file on
//! disk -- there is nothing to configure about a raw kernel device besides its
//! name, which is derived from the uevent itself.
//!
//! A `.device` *file* is still supported for the same reason systemd supports one:
//! so a user can attach extra `Wants=`/`After=`/`Before=` relations to a specific
//! device (e.g. "start my-backup.service after dev-sdb1.device shows up"), without
//! lksystem needing to know anything else about the device up front. If a uevent
//! later arrives for a device whose computed unit name matches an already-loaded
//! `.device` unit, that unit is reused instead of a new one being synthesized.
use crate::units::*;
use std::path::PathBuf;

pub fn parse_device(
    parsed_file: ParsedFile,
    path: &PathBuf,
) -> Result<ParsedDeviceConfig, ParsingErrorReason> {
    let mut install_config = None;
    let mut unit_config = None;

    for (name, section) in parsed_file {
        match name.as_str() {
            "[Unit]" => {
                unit_config = Some(parse_unit_section(section)?);
            }
            "[Install]" => {
                install_config = Some(parse_install_section(section)?);
            }
            _ => return Err(ParsingErrorReason::UnknownSection(name.to_owned())),
        }
    }

    Ok(ParsedDeviceConfig {
        common: ParsedCommonConfig {
            name: path.file_name().unwrap().to_str().unwrap().to_owned(),
            unit: unit_config.unwrap_or_else(Default::default),
            install: install_config.unwrap_or_else(Default::default),
            conditions: ParsedConditions::default(),
        },
    })
}
