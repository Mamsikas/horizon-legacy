//! Native forms describe the existing CLI; they never duplicate its operations or use a shell.
use clap::{Arg, ArgAction, Command};
use horizon_launcher::i18n::{tr, trf};
use std::ffi::OsString;

/// The native window handles sign-in directly so its URL and code stay visible.
/// Session handoff is deliberately absent: it can print the path to a secret file.
const SPECIAL: &[&str] = &[
    "mcp",
    "completions",
    "agent-config",
    "benchmark",
    "account session",
    "account sign-in",
];

#[derive(Clone, Debug, serde::Serialize)]
pub struct Action {
    pub path: Vec<String>,
    pub about: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Field {
    pub id: String,
    pub label: String,
    pub help: String,
    pub long: Option<String>,
    pub short: Option<char>,
    pub positional: Option<usize>,
    pub flag: bool,
    pub multiple: bool,
    pub required: bool,
    pub defaults: Vec<String>,
    pub choices: Vec<String>,
    pub last: bool,
}

fn visible(arg: &Arg) -> bool {
    !arg.is_hide_set() && !matches!(arg.get_id().as_str(), "help" | "version" | "json")
}

fn field(arg: &Arg) -> Field {
    let id = arg.get_id().to_string();
    Field {
        label: arg
            .get_long()
            .map(str::to_owned)
            .unwrap_or_else(|| id.replace('_', "-")),
        id,
        help: arg
            .get_long_help()
            .or(arg.get_help())
            .map(ToString::to_string)
            .unwrap_or_default(),
        long: arg.get_long().map(str::to_owned),
        short: arg.get_short(),
        positional: arg.get_index(),
        flag: matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse),
        multiple: matches!(arg.get_action(), ArgAction::Append)
            || arg.get_num_args().is_some_and(|n| n.max_values() > 1),
        required: arg.is_required_set(),
        defaults: arg
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect(),
        choices: arg
            .get_possible_values()
            .into_iter()
            .filter(|v| !v.is_hide_set())
            .map(|v| v.get_name().to_owned())
            .collect(),
        last: arg.is_last_set(),
    }
}

/// Every visible leaf operation, in the original CLI's order.
pub fn actions() -> Vec<Action> {
    let mut definition = crate::cli::command();
    // Assign positional indices and propagate global arguments before inspecting metadata.
    definition.build();
    let mut out = Vec::new();
    collect(&definition, &mut Vec::new(), &mut out);
    out
}

fn collect(command: &Command, path: &mut Vec<String>, out: &mut Vec<Action>) {
    for child in command
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
    {
        path.push(child.get_name().to_owned());
        if child.has_subcommands() {
            collect(child, path, out);
        } else if !SPECIAL.contains(&path.join(" ").as_str()) && child.get_name() != "help" {
            out.push(Action {
                path: path.clone(),
                about: child
                    .get_long_about()
                    .or(child.get_about())
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                fields: child
                    .get_arguments()
                    .filter(|a| visible(a))
                    .map(field)
                    .collect(),
            });
        }
        path.pop();
    }
}

impl Action {
    /// `None` leaves an argument at its CLI default. `Some("")` passes an explicit empty
    /// value, which existing settings and instance commands use to clear a choice.
    /// A repeated value has one argument per line, preserving spaces and Unicode verbatim.
    /// A flag is enabled by `Some("true")`; `None` or `Some("false")` leaves it absent.
    /// The result excludes the executable name and is ready for `Command::args`.
    pub fn arguments(&self, inputs: &[Option<String>]) -> Result<Vec<OsString>, String> {
        if inputs.len() != self.fields.len() {
            return Err(tr("The form changed. Select the action again.").to_owned());
        }
        let mut words: Vec<OsString> = self.path.iter().map(OsString::from).collect();
        words.push("--json".into());
        let mut positional = Vec::new();
        let mut last = Vec::new();
        let missing = self
            .fields
            .iter()
            .zip(inputs)
            .filter(|(_, input)| input.is_none())
            .filter_map(|(field, _)| field.positional)
            .collect::<Vec<_>>();
        for (field, input) in self.fields.iter().zip(inputs) {
            let Some(value) = input else {
                continue;
            };
            if field.flag {
                if value == "true" {
                    words.push(field.option()?);
                } else if value != "false" {
                    return Err(trf(
                        "{field}: choose true or false.",
                        &[("field", &field.label)],
                    ));
                }
                continue;
            }
            let values: Vec<OsString> = if field.multiple {
                value
                    .split('\n')
                    .map(|v| v.trim_end_matches('\r').into())
                    .collect()
            } else {
                vec![value.into()]
            };
            if let Some(index) = field.positional {
                // Skipping an earlier optional positional would silently change the meaning
                // of this value. Let the user enable that earlier field explicitly instead.
                if missing.iter().any(|missing| *missing < index) {
                    return Err(trf(
                        "{field}: fill the preceding positional fields first.",
                        &[("field", &field.label)],
                    ));
                }
                let target = if field.last {
                    &mut last
                } else {
                    &mut positional
                };
                target.push((index, values));
            } else {
                for value in values {
                    let mut option = field.option()?;
                    // An equals sign keeps values such as --server-thread from being parsed
                    // as launcher options, including explicitly empty values.
                    option.push("=");
                    option.push(value);
                    words.push(option);
                }
            }
        }
        positional.sort_by_key(|(index, _)| *index);
        last.sort_by_key(|(index, _)| *index);
        if !positional.is_empty() && last.is_empty() {
            words.push("--".into());
        }
        words.extend(positional.into_iter().flat_map(|(_, values)| values));
        if !last.is_empty() {
            words.push("--".into());
            words.extend(last.into_iter().flat_map(|(_, values)| values));
        }
        crate::cli::command()
            .try_get_matches_from(
                std::iter::once(OsString::from("horizon-launcher")).chain(words.clone()),
            )
            .map_err(|error| error.to_string())?;
        Ok(words)
    }
}

impl Field {
    fn option(&self) -> Result<OsString, String> {
        if let Some(long) = &self.long {
            Ok(format!("--{long}").into())
        } else if let Some(short) = self.short {
            Ok(format!("-{short}").into())
        } else {
            Err(trf(
                "{field} has no option name.",
                &[("field", &self.label)],
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(path: &str, values: &[(&str, &str)]) -> Result<Vec<OsString>, String> {
        let action = actions()
            .into_iter()
            .find(|a| a.path.join(" ") == path)
            .unwrap();
        let inputs = action
            .fields
            .iter()
            .map(|field| {
                values
                    .iter()
                    .find(|(id, _)| *id == field.id)
                    .map(|(_, value)| (*value).to_owned())
            })
            .collect::<Vec<_>>();
        action.arguments(&inputs)
    }

    fn parsed(words: Vec<OsString>) -> clap::ArgMatches {
        crate::cli::command()
            .try_get_matches_from(std::iter::once(OsString::from("horizon-launcher")).chain(words))
            .unwrap()
    }

    #[test]
    fn instance_name_is_a_single_unicode_argument() {
        let words = build("instance create", &[("name", "Мой мир с пробелами")]).unwrap();
        let matches = parsed(words);
        let create = matches
            .subcommand_matches("instance")
            .unwrap()
            .subcommand_matches("create")
            .unwrap();
        assert_eq!(
            create.get_one::<String>("name").unwrap(),
            "Мой мир с пробелами"
        );
        assert!(matches.get_flag("json"));
    }

    #[test]
    fn repeated_game_arguments_preserve_spaces_and_leading_dashes() {
        let words = build(
            "play",
            &[
                ("instance", "Мой экземпляр"),
                ("arg", "--world\nМир с пробелами\n--server-thread"),
            ],
        )
        .unwrap();
        let matches = parsed(words);
        let play = matches.subcommand_matches("play").unwrap();
        let args = play
            .get_many::<String>("arg")
            .unwrap()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(args, ["--world", "Мир с пробелами", "--server-thread"]);
    }

    #[test]
    fn pack_selection_and_flags_follow_the_original_parser() {
        let words = build("packs on", &[("pack", "Мой пак.zip"), ("kind", "shader")]).unwrap();
        let matches = parsed(words);
        let on = matches
            .subcommand_matches("packs")
            .unwrap()
            .subcommand_matches("on")
            .unwrap();
        assert_eq!(on.get_one::<String>("pack").unwrap(), "Мой пак.zip");
        let defaults = parsed(build("logs", &[("crash", "true")]).unwrap());
        let logs = defaults.subcommand_matches("logs").unwrap();
        assert!(logs.get_flag("crash"));
        assert_eq!(logs.get_one::<usize>("lines"), Some(&100));
    }

    #[test]
    fn validates_missing_conflicting_and_invalid_fields_before_spawning() {
        assert!(build("instance create", &[]).is_err());
        assert!(build(
            "prepare",
            &[("minecraft", "C:\\Моя игра"), ("download", "true")]
        )
        .is_err());
        assert!(build("logs", &[("lines", "many")]).is_err());
        assert!(build("settings", &[("value", "on")]).is_err());
    }

    #[test]
    fn empty_values_clear_settings_and_last_arguments_reach_server() {
        let matches =
            parsed(build("instance launch", &[("instance", "horizon"), ("env", "")]).unwrap());
        let launch = matches
            .subcommand_matches("instance")
            .unwrap()
            .subcommand_matches("launch")
            .unwrap();
        assert_eq!(
            launch.get_one::<String>("env").map(String::as_str),
            Some("")
        );
        let matches = parsed(
            build(
                "servers run",
                &[("server", "Мой сервер"), ("args", "status\n--json")],
            )
            .unwrap(),
        );
        let run = matches
            .subcommand_matches("servers")
            .unwrap()
            .subcommand_matches("run")
            .unwrap();
        assert_eq!(run.get_one::<String>("server").unwrap(), "Мой сервер");
        assert_eq!(
            run.get_many::<String>("args")
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["status", "--json"]
        );
    }
}
