use std::collections::HashMap;
use std::mem;
use std::path::{PathBuf, Path};
use std::fs::read_to_string;
use std::str::FromStr;

use failure::{Error, ResultExt, err_msg};
use nginx_config;
use nginx_config::ast::{self, Listen, Value, Directive, Item};
use nginx_config::{parse_directives, Pos};
use nginx_config::visitors::{replace_vars, visit_mutable};
use regex::Regex;

use nginx_config_mod::{Config, EntryPoint};
use nginx_config_mod::checks;

#[derive(StructOpt)]
pub struct Modify {

    #[structopt(parse(from_os_str))]
    file: PathBuf,

    #[structopt(short="s", long="subst-variable", name="var=value",
                help="replace variable in the config to specified value")]
    set_var: Vec<String>,

    #[structopt(long="subst-server-name", name="orig.domain=dest.domain",
                help="replace orig.domain and all names starting with it \
                      to a dest.domain (keeping prefix if needed)")]
    server_name_mapping: Vec<String>,

    #[structopt(long="subst-proxy-pass-host", name="orig.host=dest.host",
                help="replace orig.host and all names starting with it \
                      to a dest.host (keeping prefix and port). \
                      These replaces take place *before* regex replacement.")]
    proxy_pass_mapping: Vec<String>,

    #[structopt(long="regex-subst-proxy-pass", name="REGEX=SUBST",
                help="replace REGEX to a destination substitution. \
                      Destination substitution may contain capture groups \
                      '$0', '$1'...
                      Regex replace takes place *after* host replacement")]
    proxy_pass_regexes: Vec<String>,

    #[structopt(long="regex-subst-if", name="IF_REGEX=SUBST",
                help="replace REGEX in 'if' conditions (only literal value, \
                      and only in `=` or `!=` conditions). \
                      Destination substitution may contain capture groups \
                      '$0', '$1'... ")]
    if_regexes: Vec<String>,

    #[structopt(long="regex-subst-rewrite-host", name="HOST_REGEX=SUBST",
                help="replace REGEX in host for rewrite directives. \
                      This work only for `http://host` or `https://host`,
                      or `$scheme://host` rewrites not local ones.
                      Destination substitution may contain capture groups \
                      '$0', '$1'... ")]
    rewrite_host_regexes: Vec<String>,

    #[structopt(long="check-proxy-pass-hostnames", help="\
        Also check that all hostnames in proxy_pass directives can be \
        resolved. This is needed because nginx refuses to start if can't \
        resolve IP addresses. \
        Note: this might be slow. \
        ")]
    check_proxy_pass_hostnames: bool,

    #[structopt(long="regex-proxy-pass-exclude", help="\
        Exclude following hostnames for the check-proxy-pass-hostnames. \
        This regex should match all upstreams, because it's usually \
        useless to resolve them. \
        (Note: currently, we don't check upstreams present in config, \
        but we may add the feature later). \
        ")]
    proxy_pass_exclude: Vec<String>,

    #[structopt(long="expand-local-includes", help="\
        Expand non-absolute includes to their contents. \
        Include path is treated relative to the configuration file path. \
        Including works before all other directives so included contents is \
        processed normally.")]
    expand_local_includes: bool,

    #[structopt(long="allow-includes", help="\
        Add a path prefix to a list of allowed include directive prefixes. \
        Note: these includes aren't read and checked so may possibly \
        contain disallowed things if not checked on its own.",
        parse(from_os_str))]
    allow_includes: Vec<PathBuf>,

    #[structopt(long="listen", name="LISTEN",
                help="replace all listen directives to this value. \
                If used multiple times. This number of listen directives will \
                be present in each block",
                parse(try_from_str="parse_listen"))]
    listen: Vec<Listen>,

    #[structopt(long="replace-by-name", name="DIR=VALUE", help="\
        Replace any occurrence of directve named DIR with another directive, \
        specified in VALUE. For example ``expires=add_header Expires never``. \
        VALUE must be a single and fully valid directive, excluding \
        semicolon. This replacement works *after* all other replacements \
        took place. Only one rule is applied to each directive.",
        parse(try_from_str="parse_replacement"))]
    replace_by_name: Vec<(String, Item)>,
}

fn parse_listen(s: &str) -> Result<Listen, Error> {
    let text = format!("listen {};", s);
    let mut dirs = parse_directives(&text)?;
    if dirs.len() > 1 {
        bail!("unexpected semicolon");
    }
    match dirs.pop().map(|d| d.item) {
        Some(ast::Item::Listen(lst)) => Ok(lst),
        _ => bail!("Internal error when parsing listen directive"),
    }
}

fn parse_replacement(s: &str) -> Result<(String, Item), Error> {
    let mut pair = s.splitn(2, "=");
    let name = pair.next().unwrap();
    if name.len() == 0 {
        bail!("directive name must not be empty");
    }
    let dirs = match pair.next() {
        Some(x) => {
            let text = format!("{};", x);
            parse_directives(&text)?
        }
        None => bail!("no replacement directive specified"),
    };
    if dirs.len() == 0 {
        bail!("no replacement directive specified");
    }
    if dirs.len() > 1 {
        bail!("Only single replacement directive may be specified \
               (consider removing semicolon from argument)");
    }
    Ok((name.to_string(), {dirs}.pop().unwrap().item))
}

// TODO(tailhook) temporary, until we expose Value::parse
fn parse_proxy(s: &str) -> Result<Value, Error> {
    let text = format!("proxy_pass {};", s);
    let mut dirs = parse_directives(&text)?;
    if dirs.len() > 1 {
        bail!("Only single proxy_pass directive may be specified \
               (consider removing semicolon from argument)");
    }
    match dirs.pop().map(|d| d.item) {
        Some(ast::Item::ProxyPass(value)) => Ok(value),
        _ => bail!("Internal error when parsing proxy_pass directive"),
    }
}

fn relative<'x>(name: &'x str, anchor: &str) -> Option<&'x str> {
    if name.ends_with(anchor) {
        if anchor.len() == name.len() {
            return Some("");
        } else if name[..name.len() - anchor.len()].ends_with(".") {
            return Some(&name[..name.len() - anchor.len()])
        } else {
            return None
        }
    } else {
        None
    }
}

fn proxy_subst<'x>(name: &'x str, anchor: &str) -> Option<(&'x str, &'x str)> {
    let mut cur = name;
    let mut prefix = 0;
    let mut suffix = 0;
    if name.starts_with("http://") {
        cur = &cur["http://".len()..];
        prefix += "http://".len();
    } else if name.starts_with("https://") {
        cur = &cur["https://".len()..];
        prefix += "https://".len();
    } else {
        return None;
    }
    if let Some(suf) = cur.find('/') {
        suffix += cur.len() - suf;
        cur = &cur[..suf];
    }
    if let Some(suf) = cur.find(':') {
        suffix += cur.len() - suf;
        cur = &cur[..suf];
    }
    if cur.ends_with(anchor) {
        // TODO(tailhook) check for variables ?
        if anchor.len() == cur.len() {
            return Some((&name[..prefix], &name[name.len() - suffix..]));
        } else if cur[..cur.len() - anchor.len()].ends_with(".") {
            prefix += cur.len() - anchor.len();
            return Some((&name[..prefix], &name[name.len() - suffix..]));
        } else {
            return None
        }
    } else {
        None
    }
}


fn server_names(cfg: &mut Config, names: &Vec<String>)
    -> Result<(), Error>
{
    use nginx_config::ast::ServerName::*;
    let mut snames = HashMap::new();
    for item in names {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            snames.insert(orig, dest);
        } else {
            bail!("server name {:?} doesn't include substitution target \
                   (format is `orig.name=dest.example.org`)");
        }
    }
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            ast::Item::ServerName(ref mut names) => {
                for name in names {
                    match *name {
                        Exact(ref mut n) | Suffix(ref mut n) |
                        StarSuffix(ref mut n)
                        => {
                            for (orig, new) in &snames {
                                *n = if let Some(prefix) = relative(n, orig) {
                                    format!("{}{}", prefix, new)
                                } else {
                                    continue;
                                }
                            }
                        }
                        StarPrefix(_) => {}  // ingoring, warn? forbid?
                        Regex(_) => {}  // ingoring, warn? forbid?
                    }
                }
            }
            _ => {}
        }
    });
    Ok(())
}

fn proxy_pass_regexes(cfg: &mut Config, items: &Vec<String>)
    -> Result<(), Error>
{
    let mut regexes = Vec::new();
    for item in items {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            let regex = Regex::new(orig).context(orig.to_string())?;
            regexes.push((regex, dest));
        } else {
            bail!("proxy pass regex {:?} doesn't include substitution \
                target (format is `REGEX=SUBST`)");
        }
    }
    let mut err = None;
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            ast::Item::ProxyPass(ref mut value) => {
                let mut s = value.to_string();
                for (regex, repl) in &regexes {
                    s = regex.replace(s.as_ref(), *repl).to_string();
                }
                match parse_proxy(&s) {
                    Ok(x) => *value = x,
                    Err(e) => err = Some(e),
                }
            }
            _ => {}
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(())
}

fn if_regexes(cfg: &mut Config, items: &Vec<String>)
    -> Result<(), Error>
{
    use nginx_config::ast::IfCondition::*;
    let mut regexes = Vec::new();
    for item in items {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            let regex = Regex::new(orig).context(orig.to_string())?;
            regexes.push((regex, dest));
        } else {
            bail!("if regex {:?} doesn't include substitution \
                target (format is `REGEX=SUBST`)");
        }
    }
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            | ast::Item::If(ast::If { condition: Eq(_, ref mut value), .. })
            | ast::Item::If(ast::If { condition: Neq(_, ref mut value), .. })
            => {
                let mut s = value.to_string();
                for (regex, repl) in &regexes {
                    s = regex.replace(s.as_ref(), *repl).to_string();
                }
                *value = s;
            }
            _ => {}
        }
    });
    Ok(())
}

fn rewrite_host_regexes(cfg: &mut Config, items: &Vec<String>)
    -> Result<(), Error>
{
    let mut regexes = Vec::new();
    for item in items {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            let regex = Regex::new(orig).context(orig.to_string())?;
            regexes.push((regex, dest));
        } else {
            bail!("rewrite host regex {:?} doesn't include substitution \
                target (format is `REGEX=SUBST`)");
        }
    }
    let mut err = None;
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            | ast::Item::Rewrite(ast::Rewrite { ref mut replacement, .. })
            => {
                let mut s = replacement.to_string();
                if !(s.starts_with("http://") ||
                     s.starts_with("https://") ||
                     s.starts_with("$scheme://"))
                {
                    return;
                }
                let (prefix, tmp) = s.split_at(s.find('/').unwrap() + 2);
                let (host, suffix) = if let Some(end) = tmp.find('/') {
                    tmp.split_at(end)
                } else {
                    (tmp, "")
                };
                let mut host = host.to_string();
                for (regex, repl) in &regexes {
                    host = regex.replace(host.as_ref(), *repl).to_string();
                }
                let mut s = String::with_capacity(
                    prefix.len() + suffix.len() + host.len());
                s.push_str(prefix);
                s.push_str(&host);
                s.push_str(suffix);
                match Value::from_str(&s) {
                    Ok(v) => *replacement = v,
                    Err(e) => err = Some(e),
                }
            }
            _ => {}
        }
    });
    if let Some(e) = err {
        return Err(err_msg(e));
    }
    Ok(())
}

fn proxy_pass_mapping(cfg: &mut Config, names: &Vec<String>)
    -> Result<(), Error>
{
    let mut pnames = HashMap::new();
    for item in names {
        let mut pair = item.splitn(2, '=');
        let orig = pair.next().expect("first item always exists");
        if let Some(dest) = pair.next() {
            pnames.insert(orig, dest);
        } else {
            bail!("proxy pass host {:?} doesn't include substitution \
                target (format is `orig.host=dest.example.org`)");
        }
    }
    let mut err = None;
    visit_mutable(cfg.directives_mut(), |dir| {
        match dir.item {
            ast::Item::ProxyPass(ref mut value) => {
                let mut s = value.to_string();
                for (orig, new) in &pnames {
                    s = if let Some((pre, suf)) = proxy_subst(&s, orig) {
                        format!("{}{}{}", pre, new, suf)
                    } else {
                        continue;
                    }
                }
                match parse_proxy(&s) {
                    Ok(x) => *value = x,
                    Err(e) => err = Some(e),
                }
            }
            _ => {}
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(())
}

fn replace_listen(dirs: &mut Vec<Directive>, lst: &Vec<Listen>) {
    let ref is_listen = |x: &Directive| matches!(x.item, ast::Item::Listen(..));
    if let Some(pos) = dirs.iter().position(is_listen) {
        dirs.retain(|x| !is_listen(x));
        for (idx, item) in lst.iter().enumerate() {
            dirs.insert(pos+idx, Directive {
                position: Pos { line: 0, column: 0 },
                item: ast::Item::Listen(item.clone()),
            });
        }
    }
}

fn expand_local_includes(dest: &mut Vec<Directive>, path: &Path)
    -> Result<(), Error>
{
    use nginx_config::ast::Item::Include;

    let orig_len = dest.len();
    let source = mem::replace(dest, Vec::with_capacity(orig_len));
    for mut dir in source {
        let inc_path = match dir.item {
            Include(ref inc_path) => {
                let inc_path = inc_path.to_string();
                let inc_path = Path::new(&inc_path);
                if !inc_path.is_absolute() {
                    Some(path.parent()
                        .expect("file path always has parent")
                        .join(inc_path))
                } else {
                    None
                }
            }
            _ => {
                if let Some(list) = dir.item.children_mut() {
                    expand_local_includes(list, path)?;
                }
                None
            }
        };
        let inc_path = if let Some(inc_path) = inc_path  {
            inc_path
        } else {
            dest.push(dir);
            continue;
        };
        let contents = read_to_string(&inc_path)
            .map_err(|e| format_err!("error reading {:?}: {}", inc_path, e))?;
        let inc_dirs = nginx_config::parse_directives(&contents)?;
        dest.extend(inc_dirs);
    }
    Ok(())
}

pub fn run(modify: Modify) -> Result<(), Error> {
    let mut cfg = Config::partial_file(EntryPoint::Main, &modify.file)?;

    if modify.expand_local_includes {
        expand_local_includes(cfg.directives_mut(), &modify.file)?;
    }

    // vars
    let mut vars = HashMap::new();
    for item in &modify.set_var {
        let mut pair = item.splitn(2, '=');
        vars.insert(pair.next().expect("first item always exists"),
            pair.next().unwrap_or(""));
    }
    replace_vars(cfg.directives_mut(), |name| vars.get(name).map(|x| *x));

    // listen
    if modify.listen.len() > 0 {
        replace_listen(cfg.directives_mut(), &modify.listen);
        visit_mutable(cfg.directives_mut(), |dir| {
            if let Some(children) = dir.item.children_mut() {
                replace_listen(children, &modify.listen);
            }
        });
    }

    if modify.server_name_mapping.len() > 0 {
        server_names(&mut cfg, &modify.server_name_mapping)?;
    }

    if modify.proxy_pass_mapping.len() > 0 {
        proxy_pass_mapping(&mut cfg, &modify.proxy_pass_mapping)?;
    }

    if modify.proxy_pass_regexes.len() > 0 {
        proxy_pass_regexes(&mut cfg, &modify.proxy_pass_regexes)?;
    }
    if modify.if_regexes.len() > 0 {
        if_regexes(&mut cfg, &modify.if_regexes)?;
    }
    if modify.rewrite_host_regexes.len() > 0 {
        rewrite_host_regexes(&mut cfg, &modify.rewrite_host_regexes)?;
    }

    for dir in cfg.all_directives() {
        match &dir.item {
            Item::Include(path) => {
                let path = path.to_string();
                let path = Path::new(&path);
                if !modify.allow_includes.iter().any(|p| path.starts_with(p)) {
                    bail!("invalid include path {:?}, allowed: {:?}",
                        path, &modify.allow_includes);
                }
            }
            _ => {}
        }
    }

    if modify.replace_by_name.len() > 0 {
        visit_mutable(cfg.directives_mut(), |dir| {
            for (name, value) in &modify.replace_by_name {
                if dir.item.directive_name() == name {
                    dir.item = value.clone();
                    break;
                }
            }
        })
    }

    if modify.check_proxy_pass_hostnames {
        let regexes = modify.proxy_pass_exclude.iter()
            .map(|r| Regex::new(&r))
            .collect::<Result<Vec<_>, _>>()?;
        let result = checks::proxy_pass::check_selected_hostnames(&cfg, |dom| {
            !regexes.iter().any(|x| x.is_match(dom))
        });
        if let Err(errs) = result {
            for e in errs {
                error!("{}", e);
            }
            return Err(err_msg("failed to resolve some hostnames"));
        }
    }

    print!("{}", cfg);
    Ok(())
}
