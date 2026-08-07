use crate::units::*;
use std::path::PathBuf;

pub fn parse_path(
    parsed_file: ParsedFile,
    path: &PathBuf,
) -> Result<ParsedPathConfig, ParsingErrorReason> {
    let mut path_config = None;
    let mut install_config = None;
    let mut unit_config = None;

    for (name, section) in parsed_file {
        match name.as_str() {
            "[Path]" => {
                path_config = Some(parse_path_section(section)?);
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

    let path_config = match path_config {
        Some(conf) => conf,
        None => return Err(ParsingErrorReason::SectionNotFound("Path".to_owned())),
    };

    let file_name = path.file_name().unwrap().to_str().unwrap().to_owned();
    let path_config = if path_config.unit.is_none() {
        let base = file_name.trim_end_matches(".path");
        ParsedPathSection {
            unit: Some(format!("{}.service", base)),
            ..path_config
        }
    } else {
        path_config
    };

    Ok(ParsedPathConfig {
        common: ParsedCommonConfig {
            name: file_name,
            unit: unit_config.unwrap_or_else(Default::default),
            install: install_config.unwrap_or_else(Default::default),
            conditions: ParsedConditions::default(),
        },
        path: path_config,
    })
}

fn parse_path_section(
    mut section: ParsedSection,
) -> Result<ParsedPathSection, ParsingErrorReason> {
    let path_exists = match section.remove("PATHEXISTS") {
        None => None,
        Some(mut values) => {
            if values.len() == 1 {
                Some(PathBuf::from(values.remove(0).1))
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "PathExists".to_owned(),
                    super::map_tupels_to_second(values),
                ));
            }
        }
    };

    let unit = match section.remove("UNIT") {
        None => None,
        Some(mut values) => {
            if values.len() == 1 {
                Some(values.remove(0).1)
            } else {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Unit".to_owned(),
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

    if path_exists.is_none() {
        return Err(ParsingErrorReason::MissingSetting("PathExists".to_owned()));
    }

    Ok(ParsedPathSection { path_exists, unit })
}
