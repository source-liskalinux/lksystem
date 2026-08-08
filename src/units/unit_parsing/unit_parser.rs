//! Parse all supported unit types / options for these and do needed operations like matching services <-> sockets and adding implicit dependencies like
//! all sockets to socket.target

use crate::ui;

use crate::units::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub type ParsedSection = HashMap<String, Vec<(u32, String)>>;
pub type ParsedFile = HashMap<String, ParsedSection>;

pub fn parse_file(content: &str) -> Result<ParsedFile, ParsingErrorReason> {
    let mut sections = HashMap::new();
    let lines: Vec<&str> = content.split('\n').collect();
    let lines: Vec<_> = lines.iter().map(|s| s.trim()).collect();

    if lines.iter().all(|line| line.is_empty()) {
        return Ok(sections);
    }

    let mut lines_left = &lines[..];

    // remove lines before the first section
    while !lines_left.is_empty() && !lines_left[0].starts_with('[') {
        lines_left = &lines_left[1..];
    }
    if lines_left.is_empty() {
        return Ok(sections);
    }

    let mut current_section_name: String = lines_left[0].into();
    let mut current_section_lines = Vec::new();

    lines_left = &lines_left[1..];

    while !lines_left.is_empty() {
        let line = lines_left[0];

        if line.starts_with('[') {
            if sections.contains_key(&current_section_name) {
                return Err(ParsingErrorReason::SectionTooOften(
                    current_section_name.to_owned(),
                ));
            } else {
                sections.insert(
                    current_section_name.clone(),
                    parse_section(&current_section_lines),
                );
            }
            current_section_name = line.into();
            current_section_lines.clear();
        } else {
            current_section_lines.push(line);
        }
        lines_left = &lines_left[1..];
    }

    // insert last section
    if sections.contains_key(&current_section_name) {
        return Err(ParsingErrorReason::SectionTooOften(
            current_section_name.to_owned(),
        ));
    } else {
        sections.insert(current_section_name, parse_section(&current_section_lines));
    }

    Ok(sections)
}

pub fn parse_unit_file(path: &Path) -> Result<ParsedFile, ParsingErrorReason> {
    let mut merged = parse_file_from_path(path)?;

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(merged);
    };
    let override_dir = path.parent().map_or_else(
        || PathBuf::from(format!("{}.d", file_name)),
        |parent| parent.join(format!("{}.d", file_name)),
    );

    if !override_dir.exists() || !override_dir.is_dir() {
        return Ok(merged);
    }

    let mut override_files = fs::read_dir(&override_dir)
        .map_err(|e| ParsingErrorReason::FileError(Box::new(e)))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .collect::<Vec<_>>();
    override_files.sort_by(|left, right| left.path().cmp(&right.path()));

    for entry in override_files {
        let content = fs::read_to_string(entry.path()).map_err(|e| {
            ParsingErrorReason::FileError(Box::new(e))
        })?;
        let overlay = parse_file(&content)?;
        merge_parsed_files(&mut merged, &overlay);
    }

    Ok(merged)
}

fn parse_file_from_path(path: &Path) -> Result<ParsedFile, ParsingErrorReason> {
    let content = fs::read_to_string(path).map_err(|e| ParsingErrorReason::FileError(Box::new(e)))?;
    parse_file(&content)
}

fn merge_parsed_files(target: &mut ParsedFile, overlay: &ParsedFile) {
    for (section_name, section_entries) in overlay {
        let target_section = target.entry(section_name.clone()).or_insert_with(HashMap::new);
        for (key, values) in section_entries {
            target_section.insert(key.clone(), values.clone());
        }
    }
}

pub fn map_tupels_to_second<X, Y: Clone>(v: Vec<(X, Y)>) -> Vec<Y> {
    v.iter().map(|(_, scnd)| scnd.clone()).collect()
}

pub fn string_to_bool(s: &str) -> bool {
    if s.len() == 0 {
        return false;
    }

    let s_upper = &s.to_uppercase();
    let c: char = s_upper.chars().nth(0).unwrap();

    let is_num_and_one = s.len() == 1 && c == '1';
    *s_upper == *"YES" || *s_upper == *"TRUE" || is_num_and_one
}

fn parse_environment(raw_line: &str) -> Result<EnvVars, ParsingErrorReason> {
    ui::log(format!("raw line: {}", raw_line));
    let split = shlex::split(raw_line).ok_or(ParsingErrorReason::Generic(format!(
        "Could not parse cmdline: {}",
        raw_line
    )))?;
    ui::log(format!("split: {:?}", split));
    let mut vars: Vec<(String, String)> = Vec::new();

    for pair in split {
        let mut p = pair.splitn(2, '=');
        let key = p.next().unwrap_or_default().trim().to_owned();
        let val = p.next().unwrap_or_default().trim().to_owned();
        if !key.is_empty() {
            vars.push((key, val));
        }
    }

    Ok(EnvVars { vars })
}

pub fn parse_unit_section(
    mut section: ParsedSection,
) -> Result<ParsedUnitSection, ParsingErrorReason> {
    let wants = section.remove("WANTS");
    let requires = section.remove("REQUIRES");
    let after = section.remove("AFTER");
    let before = section.remove("BEFORE");
    let conflicts = section.remove("CONFLICTS");
    let description = section.remove("DESCRIPTION");

    if !section.is_empty() {
        return Err(ParsingErrorReason::UnusedSetting(
            section.keys().next().unwrap().to_owned(),
        ));
    }

    Ok(ParsedUnitSection {
        description: description.map(|x| (x[0]).1.clone()).unwrap_or_default(),
        wants: map_tupels_to_second(wants.unwrap_or_default()),
        requires: map_tupels_to_second(requires.unwrap_or_default()),
        after: map_tupels_to_second(after.unwrap_or_default()),
        before: map_tupels_to_second(before.unwrap_or_default()),
        conflicts: map_tupels_to_second(conflicts.unwrap_or_default()),
    })
}

pub fn parse_conditions(section: &mut ParsedSection) -> Result<ParsedConditions, ParsingErrorReason> {
    let path_exists = section.remove("CONDITIONPATHEXISTS");
    let path_is_directory = section.remove("CONDITIONPATHISDIRECTORY");

    let mut conditions = ParsedConditions::default();
    if let Some(values) = path_exists {
        for (_, value) in values {
            conditions.path_exists.push(PathBuf::from(value));
        }
    }
    if let Some(values) = path_is_directory {
        for (_, value) in values {
            conditions.path_is_directory.push(PathBuf::from(value));
        }
    }

    if !section.is_empty() {
        return Err(ParsingErrorReason::UnusedSetting(
            section.keys().next().unwrap().to_owned(),
        ));
    }

    Ok(conditions)
}

fn make_stdio_option(setting: &str) -> Result<StdIoOption, ParsingErrorReason> {
    if setting.starts_with("file:") {
        let p = setting.trim_start_matches("file:");
        Ok(StdIoOption::File(p.into()))
    } else if setting.starts_with("append:") {
        let p = setting.trim_start_matches("append:");
        Ok(StdIoOption::AppendFile(p.into()))
    } else {
        return Err(ParsingErrorReason::UnsupportedSetting(format!(
            "StandardOutput: {}",
            setting
        )));
    }
}

pub fn parse_exec_section(
    section: &mut ParsedSection,
) -> Result<ParsedExecSection, ParsingErrorReason> {
    let user = section.remove("USER");
    let group = section.remove("GROUP");
    let working_directory = section.remove("WORKINGDIRECTORY");
    let stdout = section.remove("STANDARDOUTPUT");
    let stderr = section.remove("STANDARDERROR");
    let supplementary_groups = section.remove("SUPPLEMENTARYGROUPS");
    let environment = section.remove("ENVIRONMENT");
    let environment_files = section.remove("ENVIRONMENTFILE");

    let user = match user {
        None => None,
        Some(mut vec) => {
            if vec.len() == 1 {
                Some(vec.remove(0).1)
            } else if vec.len() > 1 {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "User".into(),
                    super::map_tupels_to_second(vec),
                ));
            } else {
                None
            }
        }
    };

    let group = match group {
        None => None,
        Some(mut vec) => {
            if vec.len() == 1 {
                Some(vec.remove(0).1)
            } else if vec.len() > 1 {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Group".into(),
                    super::map_tupels_to_second(vec),
                ));
            } else {
                None
            }
        }
    };
    let working_directory = match working_directory {
        None => None,
        Some(mut vec) => {
            if vec.len() == 1 {
                Some(std::path::PathBuf::from(vec.remove(0).1))
            } else if vec.len() > 1 {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "WorkingDirectory".into(),
                    super::map_tupels_to_second(vec),
                ));
            } else {
                None
            }
        }
    };

    let stdout_path = match stdout {
        None => None,
        Some(mut vec) => {
            if vec.len() == 1 {
                Some(vec.remove(0).1)
            } else if vec.len() > 1 {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Standardoutput".into(),
                    super::map_tupels_to_second(vec),
                ));
            } else {
                None
            }
        }
    };
    let stdout_path = if let Some(p) = stdout_path {
        Some(make_stdio_option(&p)?)
    } else {
        None
    };

    let stderr_path = match stderr {
        None => None,
        Some(mut vec) => {
            if vec.len() == 1 {
                Some(vec.remove(0).1)
            } else if vec.len() > 1 {
                return Err(ParsingErrorReason::SettingTooManyValues(
                    "Standarderror".into(),
                    super::map_tupels_to_second(vec),
                ));
            } else {
                None
            }
        }
    };
    let stderr_path = if let Some(p) = stderr_path {
        Some(make_stdio_option(&p)?)
    } else {
        None
    };

    let supplementary_groups = match supplementary_groups {
        None => Vec::new(),
        Some(vec) => vec.iter().fold(Vec::new(), |mut acc, (_id, list)| {
            acc.extend(list.split(' ').map(|x| x.to_string()));
            acc
        }),
    };

    let environment = match environment {
        Some(vec) => {
            ui::log(format!("Env vec: {:?}", vec));
            Some(parse_environment(&vec[0].1)?)
        }
        None => None,
    };

    let environment_files = match environment_files {
        Some(vec) => vec
            .iter()
            .map(|(_, path)| std::path::PathBuf::from(path))
            .collect(),
        None => Vec::new(),
    };

    Ok(ParsedExecSection {
        user,
        group,
        working_directory,
        stderr_path,
        stdout_path,
        supplementary_groups,
        environment,
        environment_files,
    })
}

pub fn parse_install_section(
    mut section: ParsedSection,
) -> Result<ParsedInstallSection, ParsingErrorReason> {
    let wantedby = section.remove("WANTEDBY");
    let requiredby = section.remove("REQUIREDBY");

    if !section.is_empty() {
        return Err(ParsingErrorReason::UnusedSetting(
            section.keys().next().unwrap().to_owned(),
        ));
    }

    Ok(ParsedInstallSection {
        wanted_by: map_tupels_to_second(wantedby.unwrap_or_default()),
        required_by: map_tupels_to_second(requiredby.unwrap_or_default()),
    })
}

pub fn get_file_list(path: &PathBuf) -> Result<Vec<std::fs::DirEntry>, ParsingErrorReason> {
    if !path.exists() {
        return Err(ParsingErrorReason::Generic(format!(
            "Path to services does not exist: {:?}",
            path
        )));
    }
    if !path.is_dir() {
        return Err(ParsingErrorReason::Generic(format!(
            "Path to services does not exist: {:?}",
            path
        )));
    }
    let mut files: Vec<_> = match std::fs::read_dir(path) {
        Ok(iter) => {
            let files_vec = iter.fold(Ok(Vec::new()), |acc, file| {
                if let Ok(mut files) = acc {
                    match file {
                        Ok(f) => {
                            files.push(f);
                            Ok(files)
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    acc
                }
            });
            match files_vec {
                Ok(files) => files,
                Err(e) => return Err(ParsingErrorReason::FileError(Box::new(e))),
            }
        }
        Err(e) => return Err(ParsingErrorReason::FileError(Box::new(e))),
    };
    files.sort_by(|l, r| l.path().cmp(&r.path()));

    Ok(files)
}

pub fn parse_section(lines: &[&str]) -> ParsedSection {
    let mut entries: ParsedSection = HashMap::new();

    let mut entry_number = 0;
    for line in lines {
        //ignore comments
        if line.starts_with('#') {
            continue;
        }

        //check if this is a key value pair
        let pos = if let Some(pos) = line.find(|c| c == '=') {
            pos
        } else {
            continue;
        };
        let (name, value) = line.split_at(pos);

        let value = value.trim_start_matches('=');
        let value = value.trim();
        let name = name.trim().to_uppercase();
        let values: Vec<String> = value.split(',').map(|x| x.into()).collect();

        let vec = entries.entry(name).or_insert_with(Vec::new);
        for value in values {
            vec.push((entry_number, value));
            entry_number += 1;
        }
    }

    entries
}
