use std::fmt;
use std::process::{Command, ExitStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Qa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub name: &'static str,
    pub args: &'static [&'static str],
}

const QA_STEPS: [Step; 3] = [
    Step {
        name: "fmt",
        args: &["fmt", "--check"],
    },
    Step {
        name: "clippy",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    },
    Step {
        name: "test",
        args: &["test", "--workspace", "--all-targets", "--all-features"],
    },
];

#[derive(Debug)]
pub enum XtaskError {
    UnknownCommand {
        command: String,
    },
    SpawnFailed {
        step: &'static str,
        source: std::io::Error,
    },
    StepFailed {
        step: &'static str,
        status: ExitStatus,
    },
}

impl fmt::Display for XtaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand { command } => {
                write!(f, "unknown xtask command: {command} (supported: qa)")
            }
            Self::SpawnFailed { step, source } => {
                write!(f, "failed to spawn cargo step `{step}`: {source}")
            }
            Self::StepFailed { step, status } => {
                write!(f, "cargo step `{step}` failed with status {status}")
            }
        }
    }
}

impl std::error::Error for XtaskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpawnFailed { source, .. } => Some(source),
            Self::UnknownCommand { .. } | Self::StepFailed { .. } => None,
        }
    }
}

pub fn parse_task(args: &[String]) -> Result<Task, XtaskError> {
    match args.first().map(String::as_str) {
        None | Some("qa") => Ok(Task::Qa),
        Some(command) => Err(XtaskError::UnknownCommand {
            command: command.to_string(),
        }),
    }
}

pub fn qa_steps() -> &'static [Step] {
    &QA_STEPS
}

pub fn run_task(task: Task) -> Result<(), XtaskError> {
    let steps = match task {
        Task::Qa => qa_steps(),
    };

    for step in steps {
        run_step(step)?;
    }

    Ok(())
}

fn run_step(step: &Step) -> Result<(), XtaskError> {
    println!("==> cargo {}", step.args.join(" "));

    let status = Command::new("cargo")
        .args(step.args)
        .status()
        .map_err(|source| XtaskError::SpawnFailed {
            step: step.name,
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(XtaskError::StepFailed {
            step: step.name,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Task, parse_task, qa_steps};

    #[test]
    fn qa_steps_match_local_developer_gate() {
        let steps = qa_steps();

        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].name, "fmt");
        assert_eq!(steps[0].args, ["fmt", "--check"]);
        assert_eq!(steps[1].name, "clippy");
        assert_eq!(
            steps[1].args,
            [
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ]
        );
        assert_eq!(steps[2].name, "test");
        assert_eq!(
            steps[2].args,
            ["test", "--workspace", "--all-targets", "--all-features"]
        );
    }

    #[test]
    fn parse_task_defaults_to_qa() {
        assert_eq!(parse_task(&[]).expect("task"), Task::Qa);
    }

    #[test]
    fn parse_task_accepts_explicit_qa() {
        assert_eq!(parse_task(&["qa".to_string()]).expect("task"), Task::Qa);
    }

    #[test]
    fn parse_task_rejects_unknown_subcommand() {
        let err = parse_task(&["unknown".to_string()]).expect_err("unknown command");
        assert!(err.to_string().contains("unknown xtask command"));
    }
}
