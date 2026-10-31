//! Command-line arguments.

use std::ffi::OsString;
use std::path::PathBuf;

use thiserror::Error;

/// Usage text printed on argument errors.
pub const USAGE: &str =
    "usage: igor-cli [--config <path>] --provider <scripted|ID> [--script <path>] [--export <path>]
                [REQUEST...]

  --config <path>    configuration file (default: $IGOR_CONFIG, then ~/.config/igor/igor.toml)
  --provider <name>  `scripted` for an offline scripted model (needs --script), or a provider id
                     from the configuration
  --script <path>    JSON script of model turns for the scripted provider
  --export <path>    write the session's events to this file as redacted JSON Lines
  REQUEST            the first message; read as one line from stdin when omitted

After each turn, igor-cli reads a follow-up message from stdin. An empty line or end of input
ends the session.";

/// Which model provider to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderChoice {
    /// Replay a script of model turns from a file.
    Scripted {
        /// Script file.
        script: PathBuf,
    },
    /// A provider configured under this id.
    Configured(String),
}

/// Parsed arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Explicit configuration path.
    pub config: Option<PathBuf>,
    /// Model provider.
    pub provider: ProviderChoice,
    /// Redacted activity export file.
    pub export: Option<PathBuf>,
    /// Request text, when given on the command line.
    pub request: Option<String>,
}

/// The arguments could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ArgsError(pub String);

/// Parses arguments, excluding the program name.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, ArgsError> {
    let mut args = args.into_iter();
    let mut config = None;
    let mut provider = None;
    let mut script = None;
    let mut export = None;
    let mut words = Vec::new();
    while let Some(arg) = args.next() {
        let text = arg
            .to_str()
            .ok_or_else(|| ArgsError(format!("argument {arg:?} is not valid UTF-8")))?
            .to_owned();
        let mut value = |flag: &str| {
            args.next()
                .ok_or_else(|| ArgsError(format!("{flag} needs a value")))
        };
        match text.as_str() {
            "--config" => config = Some(PathBuf::from(value("--config")?)),
            "--script" => script = Some(PathBuf::from(value("--script")?)),
            "--export" => export = Some(PathBuf::from(value("--export")?)),
            "--provider" => {
                let name = value("--provider")?;
                let name = name
                    .into_string()
                    .map_err(|_| ArgsError("--provider must be valid UTF-8".to_owned()))?;
                provider = Some(name);
            }
            "--" => {
                for rest in args.by_ref() {
                    words.push(rest.to_string_lossy().into_owned());
                }
            }
            flag if flag.starts_with("--") => {
                return Err(ArgsError(format!("unknown option {flag}")));
            }
            _ => words.push(text),
        }
    }
    let provider = match (provider.as_deref(), script) {
        (Some("scripted"), Some(script)) => ProviderChoice::Scripted { script },
        (Some("scripted"), None) => {
            return Err(ArgsError("--provider scripted needs --script".to_owned()));
        }
        (Some(id), None) => ProviderChoice::Configured(id.to_owned()),
        (Some(_), Some(_)) => {
            return Err(ArgsError(
                "--script is only used with --provider scripted".to_owned(),
            ));
        }
        (None, _) => return Err(ArgsError("--provider is required".to_owned())),
    };
    let request = (!words.is_empty()).then(|| words.join(" "));
    Ok(Args {
        config,
        provider,
        export,
        request,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Args, ArgsError> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_scripted_run_with_request() -> Result<(), ArgsError> {
        let args = parse_strs(&[
            "--config",
            "igor.toml",
            "--provider",
            "scripted",
            "--script",
            "s.json",
            "--export",
            "activity.jsonl",
            "Order",
            "drinks",
        ])?;
        assert_eq!(args.config, Some(PathBuf::from("igor.toml")));
        assert_eq!(
            args.provider,
            ProviderChoice::Scripted {
                script: PathBuf::from("s.json")
            }
        );
        assert_eq!(args.export, Some(PathBuf::from("activity.jsonl")));
        assert_eq!(args.request.as_deref(), Some("Order drinks"));
        Ok(())
    }

    #[test]
    fn configured_provider_and_stdin_request() -> Result<(), ArgsError> {
        let args = parse_strs(&["--provider", "claude"])?;
        assert_eq!(
            args.provider,
            ProviderChoice::Configured("claude".to_owned())
        );
        assert_eq!(args.request, None);
        Ok(())
    }

    #[test]
    fn rejects_bad_combinations() {
        for bad in [
            &["--provider", "scripted"][..],
            &["--provider", "claude", "--script", "s.json"],
            &["request only"],
            &["--provider"],
            &["--provider", "x", "--verbose"],
            &["--provider", "x", "--export"],
        ] {
            assert!(parse_strs(bad).is_err(), "{bad:?}");
        }
    }
}
