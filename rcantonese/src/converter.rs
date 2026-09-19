// Converter — merge lexicons and produce Candidates. Port of Converter.cpp.
#![allow(dead_code)]

use crate::db::ImeDatabase;
use crate::types::*;
use crate::variants::{convert_text, CharacterStandard};

fn matches_symbol_target(item: &Lexicon, symbol: &Lexicon) -> bool {
        item.is_cantonese()
                && symbol.attached.is_some()
                && item.text == *symbol.attached.as_ref().unwrap()
                && item.romanization == symbol.romanization
}

pub fn dispatch(
        database: &ImeDatabase,
        memory: &[Lexicon],
        defined: &[Lexicon],
        texts: &[Lexicon],
        symbols: &[Lexicon],
        queried: &[Lexicon],
        romanization_form: RomanizationForm,
        standard: CharacterStandard,
) -> Vec<Candidate> {
        let mut ideal_memory = Vec::new();
        let mut not_ideal_memory = Vec::new();
        for item in memory {
                if item.is_ideal_input_memory() {
                        ideal_memory.push(item.clone());
                } else if item.is_not_ideal_input_memory() {
                        not_ideal_memory.push(item.clone());
                }
        }

        let mut chained: Vec<Lexicon> = Vec::with_capacity(queried.len() + not_ideal_memory.len());
        for item in queried {
                if ideal_memory.is_empty() || !item.is_compound() {
                        chained.push(item.clone());
                }
        }

        for item in not_ideal_memory.iter().rev() {
                let position = chained
                        .iter()
                        .position(|candidate| candidate.input_count <= item.input_count)
                        .unwrap_or(chained.len());
                chained.insert(position, item.clone());
        }

        let mut merged: Vec<Lexicon> = Vec::new();
        merged.extend(ideal_memory.iter().take(3).cloned());
        merged.extend(defined.iter().cloned());
        merged.extend(texts.iter().cloned());
        merged.extend(ideal_memory.iter().cloned());
        merged.extend(chained.iter().cloned());

        for symbol in symbols.iter().rev() {
                if let Some(position) = merged.iter().position(|item| matches_symbol_target(item, symbol)) {
                        merged.insert(position + 1, symbol.clone());
                }
        }
        transform(database, &merged, romanization_form, standard)
}

pub fn transform(database: &ImeDatabase, lexicons: &[Lexicon], romanization_form: RomanizationForm, standard: CharacterStandard) -> Vec<Candidate> {
        let mut result: Vec<Candidate> = Vec::with_capacity(lexicons.len());
        for lexicon in lexicons {
                let display_text = if lexicon.is_cantonese() {
                        Some(convert_text(database, &lexicon.text, standard))
                } else {
                        None
                };
                let composed_comment = if lexicon.lexicon_type == LexiconType::Composed
                        && lexicon.attached.as_ref().map(|a| !a.is_empty()).unwrap_or(false)
                {
                        Some(convert_text(database, lexicon.attached.as_ref().unwrap(), standard))
                } else {
                        None
                };

                let candidate = Candidate::new(lexicon.clone(), display_text, romanization_form, composed_comment);
                if !result.contains(&candidate) {
                        result.push(candidate);
                }
        }
        result
}
