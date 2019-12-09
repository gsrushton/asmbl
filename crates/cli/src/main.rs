use std::path;

use failure::Error;

use asmbl_core as core;

#[derive(Debug, failure::Fail)]
enum RunError {
    #[fail(display = "No route from context to target.")]
    NoRouteFromContextToTarget,
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
                .help(
                    "Specifies the directory where asmbl should search for \
                     the project.",
                )
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("target")
                .short("t")
                .long("target")
                .value_name("DIR")
                .help(
                    "Specifies the directory below which asmbl should \
                     generate targets.",
                )
                .takes_value(true),
        )
        .get_matches();

    let target_dir = match args.value_of("target") {
        Some(s) => path::Path::new(s).canonicalize()?,
        None => std::env::current_dir()?,
    };

    let context_dir = match args.value_of("context") {
        Some(s) => {
            let context_dir = path::Path::new(s).canonicalize()?;
            std::env::set_current_dir(&context_dir)?;
            context_dir
        }
        None => std::env::current_dir()?,
    };

    let target_prefix = pathdiff::diff_paths(&target_dir, &context_dir)
        .ok_or_else(|| RunError::NoRouteFromContextToTarget)?;

    let mut engine = core::Engine::new();
    engine.register_frontend("lua", asmbl_lua_frontend::FrontEnd::new());

    let units = engine.gather_units(&context_dir)?;

    let tasks = core::TaskList::new(&context_dir, &target_prefix, units)?;

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
