use std::io::Read;
use std::path::{PathBuf, Path};
use std::fs::File;

use errors::{ReadError, ReadEnum};
use nginx_config;
use nginx_config::ast::{Directive, Main};


pub struct Config {
    filename: Option<PathBuf>,
    ast: Ast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPoint {
    Main,
    Http,
    Server,
    Location,
}

enum Ast {
    Main(Main),
    Http(Vec<Directive>),
    Server(Vec<Directive>),
    Location(Vec<Directive>),
}

impl Config {
    pub fn partial_file(entry_point: EntryPoint, path: &Path)
        -> Result<Config, ReadError>
    {
        Ok(Config::_partial_file(entry_point, path)?)
    }

    fn _partial_file(entry_point: EntryPoint, path: &Path)
        -> Result<Config, ReadEnum>
    {
        use self::EntryPoint as E;
        use self::Ast as A;

        let mut buf = String::with_capacity(1024);
        let mut f = File::open(path).map_err(ReadEnum::Input)?;
        f.read_to_string(&mut buf).map_err(ReadEnum::Input)?;

        let ast = match entry_point {
            E::Main => A::Main(nginx_config::parse_main(&buf)?),
            E::Http => A::Http(nginx_config::parse_directives(&buf)?),
            E::Server => A::Server(nginx_config::parse_directives(&buf)?),
            E::Location => A::Location(nginx_config::parse_directives(&buf)?),
        };
        Ok(Config {
            filename: Some(path.to_path_buf()),
            ast,
        })
    }
}