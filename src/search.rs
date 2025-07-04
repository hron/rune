//! Search utilities.
use crate::core::{
    cons::Cons,
    env::Env,
    gc::{Context, Rt},
    object::{List, NIL, Object, ObjectType, OptionalFlag},
};
use anyhow::{Result, bail, ensure};
use fallible_iterator::FallibleIterator;
use fancy_regex::Regex;
use rune_macros::defun;

#[defun]
fn string_match<'ob>(
    regexp: &str,
    string: &str,
    start: Option<i64>,
    _inhibit_modify: OptionalFlag,
    env: &mut Rt<Env>,
    cx: &'ob Context,
) -> Result<Object<'ob>> {
    // TODO: implement inhibit-modify
    let re = Regex::new(&lisp_regex_to_rust(regexp))?;

    let start = start.unwrap_or(0) as usize;
    if let Some(matches) = re.captures_iter(&string[start..]).next() {
        let mut all: Vec<Object> = Vec::new();
        let matches = matches?;
        let mut groups = matches.iter();
        // TODO: match data should be char position, not byte
        while let Some(Some(group)) = groups.next() {
            all.push(group.start().into());
            all.push(group.end().into());
        }
        let match_data = crate::fns::slice_into_list(&all, None, cx);
        env.match_data.set(match_data);
        let head: &Cons = match_data.try_into().unwrap();
        Ok(head.car())
    } else {
        Ok(NIL)
    }
}

#[defun]
fn replace_match(
    newtext: &str,
    _fixedcase: OptionalFlag,
    _literal: OptionalFlag,
    string: Option<&str>,
    subexp: Option<usize>,
    env: &Rt<Env>,
    cx: &Context,
) -> Result<String> {
    // TODO: Handle newtext interpolation. Treat \ as special. See docstring for more.
    //
    // TODO: Handle automatic case adjustment
    let Some(string) = string else { bail!("replace-match for buffers not yet implemented") };
    let mut match_data = env.match_data.bind(cx).as_list()?.fallible();
    let subexp = subexp.unwrap_or(0);
    let sub_err = || format!("replace-match subexpression {subexp} does not exist");
    for _ in 0..(subexp * 2) {
        ensure!(match_data.next()?.is_some(), sub_err());
    }
    let Some(beg) = match_data.next()? else { bail!(sub_err()) };
    let Some(end) = match_data.next()? else { bail!(sub_err()) };

    // TODO: match data should be char position, not byte
    let beg: usize = beg.try_into()?;
    let end: usize = end.try_into()?;

    // replace the range beg..end in string with newtext
    let mut new_string = String::new();
    new_string.push_str(&string[..beg]);
    new_string.push_str(newtext);
    new_string.push_str(&string[end..]);
    Ok(new_string)
}

#[defun]
fn regexp_quote(string: &str) -> String {
    let mut quoted = String::new();
    for ch in string.chars() {
        if let '[' | '*' | '.' | '\\' | '?' | '+' | '^' | '$' = ch {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted
}

pub(crate) fn lisp_regex_to_rust(regexp: &str) -> String {
    let mut norm_regex = String::new();
    let mut chars = regexp.char_indices();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            // Invert the escaping of parens. i.e. \( => ( and ( => \(
            '(' | ')' | '{' | '}' => {
                norm_regex.push('\\');
                norm_regex.push(ch);
            }
            '\\' => match chars.next() {
                Some((_, c @ '('..=')' | c @ '{' | c @ '}')) => norm_regex.push(c),
                Some((_, '`')) => norm_regex += "\\A",
                Some((_, '\'')) => norm_regex += "\\z",
                Some((_, c)) => {
                    norm_regex.push('\\');
                    norm_regex.push(c);
                }
                None => norm_regex.push('\\'),
            },
            '[' => {
                let word = "[:word:]";
                if regexp[idx..].starts_with(word) {
                    chars.nth(word.len() - 2);
                    norm_regex.push_str("a-zA-Z");
                } else {
                    norm_regex.push('[');
                }
            }
            c => norm_regex.push(c),
        }
    }
    norm_regex
}

#[defun]
fn match_data<'ob>(
    integer: OptionalFlag,
    reuse: OptionalFlag,
    reseat: OptionalFlag,
    env: &Rt<Env>,
    cx: &'ob Context,
) -> Result<Object<'ob>> {
    ensure!(integer.is_none(), "match-data integer field is not implemented");
    ensure!(reuse.is_none(), "match-data reuse field is not implemented");
    ensure!(reseat.is_none(), "match-data reseat field is not implemented");
    Ok(env.match_data.bind(cx))
}

#[defun]
fn set_match_data<'ob>(list: List, _reseat: OptionalFlag, env: &mut Rt<Env>) -> Object<'ob> {
    // TODO: add reseat when markers implemented
    let obj: Object = list.into();
    env.match_data.set(obj);
    NIL
}

#[defun]
fn match_beginning<'ob>(subexp: usize, env: &Rt<Env>, cx: &'ob Context) -> Result<Object<'ob>> {
    let list = env.match_data.bind(cx).as_list()?;
    Ok(list.fallible().nth(subexp)?.unwrap_or_default())
}

#[defun]
fn match_end<'ob>(subexp: usize, env: &Rt<Env>, cx: &'ob Context) -> Result<Object<'ob>> {
    let list = env.match_data.bind(cx).as_list()?;
    Ok(list.fallible().nth(subexp + 1)?.unwrap_or_default())
}

#[defun]
#[expect(non_snake_case)]
fn match_data__translate(n: i64, env: &Rt<Env>, cx: &Context) -> Result<()> {
    let search_regs: List = env.match_data.bind(cx).try_into()?;
    for reg in search_regs.conses() {
        let reg = reg?;
        if let ObjectType::Int(old) = reg.car().untag() {
            reg.set_car((old + n).into())?;
        } else {
            bail!("match data was not int");
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::core::gc::RootSet;
    use rune_core::macros::root;

    use super::*;

    #[test]
    fn lisp_regex() {
        assert_eq!(lisp_regex_to_rust("foo"), "foo");
        assert_eq!(lisp_regex_to_rust("\\foo"), "\\foo");
        assert_eq!(lisp_regex_to_rust("\\(foo\\)"), "(foo)");
        assert_eq!(lisp_regex_to_rust("(foo)"), "\\(foo\\)");
        assert_eq!(lisp_regex_to_rust("\\`"), "\\A");
        assert_eq!(lisp_regex_to_rust("\\'"), "\\z");
        assert_eq!(lisp_regex_to_rust("[[:word:]]"), "[a-zA-Z]");
        assert_eq!(lisp_regex_to_rust("[[:word:]_]"), "[a-zA-Z_]");
    }

    #[test]
    fn test_replace_match() {
        let roots = &RootSet::default();
        let cx = &mut Context::new(roots);
        root!(env, new(Env), cx);
        let string = "foo bar baz";
        let newtext = "quux";
        string_match("bar", string, None, None, env, cx).unwrap();
        let result = replace_match(newtext, None, None, Some(string), None, env, cx).unwrap();
        assert_eq!(result, "foo quux baz");
    }
}
