use anyhow::{Result, bail};

use crate::session::{RawCredentials, normalize_cookie_d};

pub fn prompt() -> Result<RawCredentials> {
    println!("Ouvre app.slack.com dans ton navigateur, puis les DevTools :");
    println!(
        "  • token  : dans la console, \
         JSON.parse(localStorage.localConfig_v2).teams[location.pathname.match(/client\\/(\\w+)/)[1]].token"
    );
    println!("  • cookie : Application → Cookies → https://app.slack.com → valeur de « d »");
    println!();

    let token = rpassword::prompt_password("Token (xoxc-…) : ")?;
    let token = token.trim().trim_matches('"').to_string();
    if !token.starts_with("xoxc-") {
        bail!("le token doit commencer par xoxc-");
    }

    let cookie_d = rpassword::prompt_password("Cookie d (xoxd-…) : ")?;
    if !normalize_cookie_d(&cookie_d).starts_with("xoxd-") {
        bail!("le cookie d doit commencer par xoxd-");
    }

    Ok(RawCredentials {
        cookie_d,
        tokens: vec![token],
    })
}
