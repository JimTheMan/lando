#[macro_use]
extern crate lando;

use lando::{LambdaContext, Request, Result};

#[lando]
pub fn example<'a>(_: Request, _: LambdaContext) -> Result<&'a str> {
    Ok("👋  well hello there. What have we here?")
}
