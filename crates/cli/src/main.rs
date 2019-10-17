use std::{env, fs, path};

use failure::Error;

use asmbl_core as core;
use asmbl_utils as utils;

#[derive(Debug, failure::Fail)]
enum ProjectLoadError {
    #[fail(display = "Unable to find project definition.")]
    NoSuchProject,
}

fn find_project(dir: &path::Path) -> Result<(path::PathBuf, core::ProjectConfig), Error> {
    match fs::File::open(dir.join("asmbl.toml")) {
        Ok(file) => Ok((
            dir.to_path_buf(),
            toml::from_str(&utils::io::read_file(file)?)?,
        )),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                if let Some(parent) = dir.parent() {
                    find_project(parent)
                } else {
                    Err(Error::from(ProjectLoadError::NoSuchProject))
                }
            } else {
                Err(Error::from(err))
            }
        }
    }
}

fn run() -> Result<(), Error> {
    let args = clap::App::new("asmbl")
        .version("0.1.0")
        .about("Does great things")
        .author("G. Rushton")
        .arg(
            clap::Arg::with_name("context")
                .short("c")
                .long("context")
                .value_name("DIR")
                .help("Instructs asmbl where to look for a project")
                .takes_value(true),
        )
        .get_matches();

    let context = args.value_of("context").map(|s| path::Path::new(s));

    let (dir, config) = if let Some(context) = context {
        find_project(&context)?
    } else {
        find_project(&env::current_dir()?)?
    };

    let mut engine = core::Engine::new();
    engine.register_frontend("lua", asmbl_lua_frontend::FrontEnd::new());

    let tasks = engine.gather_tasks(dir, config)?;

    for (_handle, task) in tasks.retain_out_of_date()? {
        let mut cmd = task.prepare()?;
        println!("{:?}", cmd);
        cmd.spawn()?.wait()?;
    }

    Ok(())
}

fn main() {
    if let Err(err) = run() {
        for cause in err.iter_chain() {
            println!("{}", cause);
        }
        std::process::exit(1)
    }
}
