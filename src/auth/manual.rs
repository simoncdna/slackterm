use anyhow::{Result, bail};

use crate::session::{RawCredentials, normalize_cookie_d};

pub fn prompt() -> Result<RawCredentials> {
    println!("Open app.slack.com in your browser, then its DevTools:");
    println!(
        "  • token:  in the console, \
         JSON.parse(localStorage.localConfig_v2).teams[location.pathname.match(/client\\/(\\w+)/)[1]].token"
    );
    println!("  • cookie: Application → Cookies → https://app.slack.com → value of \"d\"");
    println!();

    let token = rpassword::prompt_password("Token (xoxc-…): ")?;
    let token = token.trim().trim_matches('"').to_string();
    if !token.starts_with("xoxc-") {
        bail!("the token must start with xoxc-");
    }

    let cookie_d = rpassword::prompt_password("d cookie (xoxd-…): ")?;
    if !normalize_cookie_d(&cookie_d).starts_with("xoxd-") {
        bail!("the d cookie must start with xoxd-");
    }

    Ok(RawCredentials {
        cookie_d,
        tokens: vec![token],
    })
}
