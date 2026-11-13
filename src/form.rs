use std::collections::BTreeMap;

fn decode(input: &str) -> Result<String, String> {
    let mut output = Vec::new();
    let mut bytes = input.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'+' => output.push(b' '),
            b'%' => {
                let a = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or("Invalid form encoding")?;
                let b = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or("Invalid form encoding")?;
                output.push((a * 16 + b) as u8);
            }
            b => output.push(b),
        }
    }
    String::from_utf8(output).map_err(|_| "Invalid form text".into())
}
pub(crate) fn form(body: &str) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for pair in body.split('&') {
        let (key, value) = pair.split_once('=').ok_or("Invalid form field")?;
        if result.len() >= 16 || result.insert(decode(key)?, decode(value)?).is_some() {
            return Err("Invalid or duplicate form fields".into());
        }
    }
    Ok(result)
}
