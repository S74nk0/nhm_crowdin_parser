use std::collections::{HashMap, BTreeMap};
use std::string::String;
use serde::{Deserialize, Serialize};
use anyhow::{Result, Ok};
use std::io::prelude::*;
use std::io::BufReader;
use std::fs::{File, self, DirEntry};
use std::path::{Path, PathBuf};
use clap::Parser;

static TRANSLATIONS_JSON: &str = "translations.json";
static CROWDIN_DIR: &str = "crowdin";

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Input path for transforming
    #[clap(short, long, value_parser, default_value = TRANSLATIONS_JSON)]
    input: String,

    /// Otput path for saving transformation
    #[clap(short, long, value_parser, default_value = CROWDIN_DIR)]
    output: String,

    /// Inverse
    #[clap(short, long)]
    reverse: bool,
}

#[derive(Deserialize)]
struct TranslationFile {
    #[serde(rename = "Languages")]
    languages: HashMap<String, String>,
    #[serde(rename = "Translations")]
    translations: HashMap<String, HashMap<String, String>>,
}

impl TranslationFile {
    fn consume(self) -> (Vec<String>, Vec<HashMap<String, String>>) {
        let langs: Vec<String> = self.languages.into_keys().collect();
        let sentences: Vec<HashMap<String, String>> = self.translations
        .into_iter()
        .map(|(en_sentence, mut translations)| {
            translations.entry("en".into()).or_insert_with(|| en_sentence);
            translations
        })
        .collect();
        let mut sentences = sentences;
        sentences.sort_by(|a,b| {
            let (a_s, b_s) = (a.get("en").unwrap(), b.get("en").unwrap());
            a_s.cmp(b_s)
        });
        (langs, sentences)
    }
}

fn nhm_translations_to_crowdin(translations_path: &str, out_dir: &str) -> Result<()> {
    fn key_str(i: usize, max: usize) -> String {
        let d1 = i.to_string().len();
        let d2 = max.to_string().len();
        let zeros: String = (0..d2-d1).map(|_| '0').collect();
        format!("k_{}{}", zeros, i)
    }

    let read_translations_json = || -> Result<(Vec<String>, Vec<HashMap<String, String>>)> {
        let f = File::open(translations_path)?;
        let reader = BufReader::new(f);
        let tr_file: TranslationFile = serde_json::from_reader(reader)?;
        Ok(tr_file.consume())
    };
    let (langs, sentences) = read_translations_json()?;

    let max = sentences.capacity();
    let create_lang_file = |lang: &str| -> Result<()> {
        let mut crowdin: BTreeMap<String, String> = BTreeMap::new();
        
        for (i, sentence) in sentences.iter().enumerate() {
            let s = sentence.get(lang).unwrap_or(&"".to_owned()).to_string();
            crowdin.insert(key_str(i, max), s);
        }
        let out_path = format!("{}/tr_{}.json", out_dir, &lang);
        let out_path = Path::new(&out_path);
        fs::create_dir_all(&out_path.parent().unwrap())?;

        let mut file = File::create(&out_path)?;
        file.write_all(serde_json::to_string_pretty(&crowdin)?.as_bytes())?;
        file.sync_all()?;
        Ok(())
    };
    
    for lang in langs.iter() {
        create_lang_file(lang)?;
    }
    Ok(())
}

fn crowdin_to_nhm_translations(crowdin_dir: &str, out_translations_path: &str) -> Result<()> {
    fn maybe_lang_key(d: &DirEntry) -> Option<(String, PathBuf)> {
        fn is_file(d: &DirEntry) -> bool { d.file_type().map_or(false,|d| d.is_file()) }
        fn get_file_name(d: &DirEntry) -> String { d.file_name().into_string().ok().unwrap_or("".to_string()) }
        fn is_lang_file(f: &str) -> bool { f.starts_with("tr_") && f.ends_with(".json") }
        fn lang_key(f: &str) -> String { f.replace("tr_", "").replace(".json", "").to_string() }
        if is_file(&d) {
            let file_name = get_file_name(&d);
            if is_lang_file(&file_name) {
                return Some((lang_key(&file_name), d.path()))
            }
        }
        None
    }

    fn to_language(lang: &str) -> String {
        let ret = match lang {
            "en" => "English",
            "ru" => "Русский (Unofficial)",
            "es" => "Español (Unofficial)",
            "pt" => "Português (Unofficial)",
            "bg" => "Български (Unofficial)",
            "it" => "Italiano (Unofficial)",
            "pl" => "Polski (Unofficial)",
            "zh_cn" => "简体中文 (Unofficial)",
            "ro" => "Română (Unofficial)",
            _ => "LANG_STUB",
        };
        ret.to_string()
    }

    fn read_crowdin_translations_json(translations_path: &str) -> Result<BTreeMap<String, String>> {
        let f = File::open(translations_path)?;
        let reader = BufReader::new(f);
        let tr_file: BTreeMap<String, String> = serde_json::from_reader(reader)?;
        Ok(tr_file)
    }

    let crowdin_lang_files: Vec<(String, PathBuf)> = fs::read_dir(crowdin_dir)?
    .filter_map(|f| f.ok())
    .filter_map(|d| maybe_lang_key(&d))
    .collect();

    let mut langs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (lang, path) in crowdin_lang_files {
        let words = read_crowdin_translations_json(path.to_str().unwrap())?;
        langs.insert(lang, words);
    }
    let languages: HashMap<String, String> = langs.keys().map(|l| (l.to_owned(), to_language(l))).collect();
    let mut translations: BTreeMap<&String, BTreeMap<&String, &String>> = BTreeMap::new();
    let en = langs.remove_entry("en").unwrap().1;
    for key in en.keys() {
        let en_word = en.get(key).unwrap();
        translations.insert(en_word, BTreeMap::new());
        let word_translations = translations.get_mut(en_word).unwrap();

        for lang in langs.keys() {
            if let Some(tr_word) = langs.get(lang).unwrap().get(key) {
                if tr_word.ne("") {
                    word_translations.insert(lang, tr_word);
                }
            }
        }
    }

    #[derive(Serialize)]
    struct SaveFile<'a> {
        #[serde(rename = "Languages")]
        languages: HashMap<String, String>,
        #[serde(rename = "Translations")]
        translations: BTreeMap<&'a String, BTreeMap<&'a String, &'a String>>,
    }

    let save_file = SaveFile {
        languages: languages,
        translations: translations,
    };

    let mut file = File::create(out_translations_path)?;
    file.write_all(serde_json::to_string_pretty(&save_file)?.as_bytes())?;
    file.sync_all()?;

    Ok(())
}

fn transform_default_args(reverse: bool, input: &str, output: &str) -> Result<(String, String)> {
    if reverse && input.eq(TRANSLATIONS_JSON) && output.eq(CROWDIN_DIR) {
        let out_path = Path::new(&output);
        let tr_path = out_path.join(&TRANSLATIONS_JSON).to_str().unwrap().to_owned();
        Ok((output.to_owned(), tr_path))
    }
    else {
        Ok((input.to_owned(), output.to_owned()))
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let (input, output) = transform_default_args(args.reverse, &args.input, &args.output)?;
    println!("{} {} {}", args.reverse, input, output);
    if args.reverse {
        crowdin_to_nhm_translations(&input, &output)?;
    }
    else {
        nhm_translations_to_crowdin(&input, &output)?;
    }
    Ok(())
}

