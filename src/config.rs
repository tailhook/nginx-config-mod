use std::io::Read;
use std::fmt;
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

    pub fn directives(&self) -> &[Directive] {
        use self::Ast::*;
        match self.ast {
            Main(ref ast) => &ast.directives,
            Http(ref dirs) | Server(ref dirs) | Location(ref dirs) => dirs,
        }
    }
    pub fn directives_mut(&mut self) -> &mut Vec<Directive> {
        use self::Ast::*;
        match self.ast {
            Main(ref mut ast) => &mut ast.directives,
            Http(ref mut dirs) | Server(ref mut dirs) | Location(ref mut dirs)
            => dirs,
        }
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Ast::*;
        match self.ast {
            Main(ref ast) => write!(f, "{}", ast),
            Http(ref dirs) | Server(ref dirs) | Location(ref dirs)
            => {
                if dirs.len() > 0 {
                    write!(f, "{}", &dirs[0])?;
                    for d in &dirs[1..] {
                        write!(f, "\n{}", d)?;
                    }
                }
                Ok(())
            }
        }
    }
}
