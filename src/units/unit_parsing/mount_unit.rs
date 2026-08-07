use crate::units::*;
use std::path::PathBuf;

pub fn parse_mount(
    parsed_file: ParsedFile,
    path: &PathBuf,
) -> Result<ParsedMountConfig, ParsingErrorReason> {
    let mut mount_config = None;
    let mut install_config = None;
    let mut unit_config = None;

    for (name, section) in parsed_file {
        match name.as_str() {
            "[Mount]" => {
                mount_config = Some(parse_mount_section(section)?);
            }
            "[Unit]" => {
                unit_config = Some(parse_unit_section(section)?);
            }
            "[Install]" => {
                install_config = Some(parse_install_section(section)?);
            }
            _ => return Err(ParsingErrorReason::UnknownSection(name.to_owned())),
        }
    }

    let mount_config = match mount_config {
        Some(conf) => conf,
        None => return Err(ParsingErrorReason::SectionNotFound("Mount".to_owned())),
    };

    let file_name = path.file_name().unwrap().to_str().unwrap().to_owned();

    Ok(ParsedMountConfig {
        common: ParsedCommonConfig {
            name: file_name,
            unit: unit_config.unwrap_or_else(Default::default),
            install: install_config.unwrap_or_else(Default::default),
            conditions: ParsedConditions::default(),
        },
        mount: mount_config,
    })
}

fn parse_mount_section(
    mut section: ParsedSection,
) -> Result<ParsedMountSection, ParsingErrorReason> {
    let what = match section.remove("WHAT") {
        None => None,
        Some(mut values) => {
            if values.len() == 1 {
                Some(PathBuf::from(values.remove(0).1))
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "What".to_owned(),
                    super::map_tupels_to_second(values),
                ));
            }
        }
    };

    let where_path = match section.remove("WHERE") {
        None => None,
        Some(mut values) => {
            if values.len() == 1 {
                Some(PathBuf::from(values.remove(0).1))
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Where".to_owned(),
                    super::map_tupels_to_second(values),
                ));
            }
        }
    };

    let fstype = match section.remove("TYPE") {
        None => None,
        Some(mut values) => {
            if values.len() == 1 {
                Some(values.remove(0).1)
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Type".to_owned(),
                    super::map_tupels_to_second(values),
                ));
            }
        }
    };

    let options = match section.remove("OPTIONS") {
        None => None,
        Some(mut values) => {
            if values.len() == 1 {
                Some(values.remove(0).1)
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Options".to_owned(),
                    super::map_tupels_to_second(values),
                ));
            }
        }
    };

    if !section.is_empty() {
        return Err(ParsingErrorReason::UnusedSetting(
            section.keys().next().unwrap().to_owned(),
        ));
    }

    if what.is_none() {
        return Err(ParsingErrorReason::MissingSetting("What".to_owned()));
    }
    if where_path.is_none() {
        return Err(ParsingErrorReason::MissingSetting("Where".to_owned()));
    }

    Ok(ParsedMountSection {
        what,
        where_path,
        fstype,
        options,
    })
}
