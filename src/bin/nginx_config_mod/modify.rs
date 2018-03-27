use std::collections::HashMap;
use std::path::PathBuf;

use nginx_config_mod::{Config, EntryPoint};
use nginx_config::parse_directives;
use nginx_config::ast::{self, Listen};
use nginx_config::visitors::{replace_vars, visit_mutable};

use failure::Error;

#[derive(StructOpt)]
pub struct Modify {

    #[structopt(parse(from_os_str))]
    file: PathBuf,

    #[structopt(short="s", long="subst-variable", name="var=value",
                help="replace variable in the config to specified value")]
    set_var: Vec<String>,
    #[structopt(long="listen", name="LISTEN",
                help="replace all listen directives to this value",
                parse(try_from_str="parse_listen"))]
    listen: Option<Listen>,
}

fn parse_listen(s: &str) -> Result<Listen, Error> {
    let text = format!("listen {};", s);
    let mut dirs = parse_directives(&text)?;
    if dirs.len() > 1 {
        bail!("Only single listen directive may be specified \
               (consider removing semicolon from argument)");
    }
    match dirs.pop().map(|d| d.item) {
        Some(ast::Item::Listen(lst)) => Ok(lst),
        _ => bail!("Internal error when parsing listen directive"),
    }
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
    if let Some(new_listen) = modify.listen {
        visit_mutable(cfg.directives_mut(), |dir| {
            match dir.item {
                ast::Item::Listen(ref mut lst) => *lst = new_listen.clone(),
                _ => {}
            }
        });
    }
    print!("{}", cfg);
    Ok(())
}
