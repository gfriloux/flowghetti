use clap::Parser;
use flowghetti::cli::{self, Cli};
use flowghetti::render::ThemeChoice;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let theme = match cli.theme {
        cli::Theme::Light => ThemeChoice::Light,
        cli::Theme::Dark => ThemeChoice::Dark,
    };
    match flowghetti::run(
        &cli.dir,
        &cli.rankdir,
        theme,
        cli.legend,
        cli.title.as_deref(),
    ) {
        Ok(dot) => {
            print!("{dot}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("flowghetti: {err}");
            ExitCode::FAILURE
        }
    }
}
