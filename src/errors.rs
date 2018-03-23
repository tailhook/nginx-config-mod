use std::io;
use nginx_config::ParseError;

#[derive(Debug, Fail)]
#[fail(display="{}", _0)]
pub struct ReadError(ReadEnum);

#[derive(Debug, Fail)]
pub(crate) enum ReadEnum {
    #[fail(display="error reading input: {}", _0)]
    Input(#[fail(cause)] io::Error),
    #[fail(display="syntax error: {}", _0)]
    Syntax(#[fail(cause)] ParseError),
}

impl From<ReadEnum> for ReadError {
    fn from(x: ReadEnum) -> ReadError {
        ReadError(x)
    }
}


impl From<ParseError> for ReadEnum {
    fn from(x: ParseError) -> ReadEnum {
        ReadEnum::Syntax(x)
    }
}
