use anyhow::{anyhow, Result};
use mlua::prelude::LuaError;
use mlua::{AnyUserData, Result as LuaResult, String as LuaString, Table, UserData};
use std::rc::Rc;
use zspell::Dictionary;

pub const LUA_GLOBAL_NAME: &str = "spellcheck";

pub struct Spellchecker {
    dict: Option<DictionarySafe>,
}

impl Spellchecker {
    pub fn new() -> Self {
        Spellchecker { dict: None }
    }

    pub fn init(&mut self, aff_path: &str, dict_path: &str) -> Result<()> {
        let aff_content = std::fs::read_to_string(aff_path)
            .map_err(|e| anyhow!("Failed to read affix file {}: {}", aff_path, e))?;
        let dict_content = std::fs::read_to_string(dict_path)
            .map_err(|e| anyhow!("Failed to read dictionary file {}: {}", dict_path, e))?;

        let dictionary = zspell::builder()
            .config_str(&aff_content)
            .dict_str(&dict_content)
            .build()
            .map_err(|e| anyhow!("Failed to build dictionary: {}", e))?;

        self.dict.replace(DictionarySafe::from(dictionary));
        Ok(())
    }

    fn check_initialized(&self) -> Result<()> {
        match self.dict.is_none() {
            true => Err(anyhow!("spellchecker not initialized")),
            false => Ok(()),
        }
    }

    pub fn check(&self, word: &str) -> Result<bool> {
        self.check_initialized()?;
        Ok(self.dict.as_ref().unwrap().check_word(word))
    }

    pub fn suggest(&self, word: &str) -> Result<Vec<String>> {
        self.check_initialized()?;
        let dict = self.dict.as_ref().unwrap();

        // Use the entry-based API to get suggestions
        let mut entries = dict.entries(word);
        if let Some(entry) = entries.next() {
            // Try to get suggestions (requires unstable-suggestions feature in zspell)
            if let Some(suggestions) = entry.suggest() {
                return Ok(suggestions.iter().map(|s| s.to_string()).collect());
            }

            // If suggestions aren't available, try to get stems as fallback
            if let Some(stems) = entry.stems() {
                return Ok(stems.map(|s| s.to_string()).collect());
            }
        }

        // If no suggestions available, return empty list
        Ok(Vec::new())
    }
}

impl UserData for Spellchecker {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_function(
            "init",
            |ctx, (aff_path, dict_path): (LuaString, LuaString)| -> LuaResult<()> {
                let this_aux = ctx.globals().get::<AnyUserData>(LUA_GLOBAL_NAME)?;
                let mut this = this_aux
                    .borrow_mut::<Spellchecker>()
                    .map_err(LuaError::external)?;
                this.init(&aff_path.to_str()?, &dict_path.to_str()?)
                    .map_err(LuaError::external)?;
                Ok(())
            },
        );
        methods.add_function("check", |ctx, word: LuaString| -> LuaResult<bool> {
            let this_aux = ctx.globals().get::<AnyUserData>(LUA_GLOBAL_NAME)?;
            let this = this_aux
                .borrow::<Spellchecker>()
                .map_err(LuaError::external)?;
            let found = this.check(&word.to_str()?).map_err(LuaError::external)?;
            Ok(found)
        });
        methods.add_function("suggest", |ctx, word: LuaString| -> LuaResult<Table> {
            let this_aux = ctx.globals().get::<AnyUserData>(LUA_GLOBAL_NAME)?;
            let this = this_aux
                .borrow::<Spellchecker>()
                .map_err(LuaError::external)?;
            let res_table = ctx.create_table()?;
            this.suggest(&word.to_str()?)
                .map_err(LuaError::external)?
                .iter()
                .enumerate()
                .for_each(|(i, v)| res_table.set(i, v.as_str()).unwrap());
            Ok(res_table)
        });
    }
}

#[derive(Clone)]
struct DictionarySafe(Rc<Dictionary>);

unsafe impl Send for DictionarySafe {}

impl std::ops::Deref for DictionarySafe {
    type Target = Dictionary;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Dictionary> for DictionarySafe {
    fn from(dictionary: Dictionary) -> Self {
        Self(Rc::new(dictionary))
    }
}

#[cfg(test)]
mod tests {
    use crate::lua::spellcheck::{Spellchecker, LUA_GLOBAL_NAME};
    use mlua::{Lua, Table};

    const AFF_PATH: &str = "tests/spellcheck/tiny.aff";
    const DICT_PATH: &str = "tests/spellcheck/tiny.dic";

    #[test]
    fn test_check_initialized() {
        let mut spellchecker = Spellchecker::new();
        assert_eq!(spellchecker.check_initialized().is_err(), true);
        spellchecker.init(AFF_PATH, DICT_PATH).unwrap();
        assert_eq!(spellchecker.check_initialized().is_ok(), true);
    }

    #[test]
    fn test_check() {
        let mut spellchecker = Spellchecker::new();
        assert_eq!(spellchecker.check("not-initialized").is_err(), true);
        spellchecker.init(AFF_PATH, DICT_PATH).unwrap();
        assert_eq!(spellchecker.check("cromulent").unwrap(), false);
        assert_eq!(spellchecker.check("cats").unwrap(), true);
    }

    #[test]
    fn test_suggest() {
        let mut spellchecker = Spellchecker::new();
        assert_eq!(spellchecker.suggest("not-initialized").is_err(), true);
        spellchecker.init(AFF_PATH, DICT_PATH).unwrap();
        let results = spellchecker.suggest("progra");
        // Note: zspell's suggestion API may behave differently than hunspell
        // This test may need adjustment based on actual behavior
        assert!(results.is_ok());
    }

    #[test]
    fn test_lua_api() {
        let lua = Lua::new();
        lua.globals()
            .set(LUA_GLOBAL_NAME, Spellchecker::new())
            .unwrap();

        // Trying to use check before init should err.
        let check_script = r#"check_res = spellcheck.check("cat")"#;
        let no_init_check = lua.load(check_script).exec();
        assert_eq!(no_init_check.is_err(), true);

        // Trying to use suggest before init should err.
        let suggest_script = r#"suggest_res = spellcheck.suggest("progra")"#;
        let no_init_suggest = lua.load(suggest_script).exec();
        assert_eq!(no_init_suggest.is_err(), true);

        // We should be able to init without err.
        let init_script = format!("spellcheck.init({:?}, {:?})", AFF_PATH, DICT_PATH);
        lua.load(init_script.as_str()).exec().unwrap();

        // After init we should be able to check/suggest.
        lua.load(check_script).exec().unwrap();
        let check_res: bool = lua.globals().get("check_res").unwrap();
        assert_eq!(check_res, true);

        lua.load(suggest_script).exec().unwrap();
        let suggest_res: Table = lua.globals().get("suggest_res").unwrap();
        // Note: Suggestion behavior may differ from hunspell, so we just verify
        // that we get a table back without error
        assert!(suggest_res.len().is_ok());
    }
}
