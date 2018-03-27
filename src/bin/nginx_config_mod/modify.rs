use std::collections::HashMap;
use std::path::PathBuf;

use nginx_config_mod::{Config, EntryPoint};
use nginx_config::visitors::{replace_vars};

use failure::Error;


#[derive(StructOpt)]
pub struct Modify {

    #[structopt(parse(from_os_str))]
    file: PathBuf,

    #[structopt(short="s", long="subst-variable", name="var=value",
                help="replace variable in the config to specified value")]
    set_var: Vec<String>,
}


pub fn run(modify: Modify) -> Result<(), Error> {
    let mut cfg = Config::partial_file(EntryPoint::Main, &modify.file)?;
    let mut vars = HashMap::new();
    for item in &modify.set_var {
        let mut pair = item.splitn(2, '=');
        vars.insert(pair.next().expect("first item always exists"),
            pair.next().unwrap_or(""));
    }
    replace_vars(cfg.directives_mut(), |name| vars.get(name).map(|x| *x));
    print!("{}", cfg);
    Ok(())
}
