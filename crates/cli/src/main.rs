use std::{env, path};

use failure::Error;

use asmbl_core as core;

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

    let context = args
        .value_of("context")
        .map_or_else(|| env::current_dir(), |s| Ok(path::PathBuf::from(s)))?;

    let mut engine = core::Engine::new();
    engine.register_frontend("lua", asmbl_lua_frontend::FrontEnd::new());

    let units = engine.gather_units(&context)?;

    let tasks = core::TaskList::new(units);

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
