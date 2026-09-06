use crate::domain::{LanguageId, LanguageRegistry};

pub struct LanguagePickerHelper;

impl LanguagePickerHelper {
    pub fn create_source_model(registry: &LanguageRegistry) -> (gtk4::StringList, Vec<LanguageId>) {
        let string_list = gtk4::StringList::new(&[]);
        let mut ids = Vec::new();

        // Add Auto Detect as the first option
        string_list.append("✨ Auto Detect");
        ids.push(LanguageId::auto());

        for lang in registry.list() {
            if lang.transcription_supported {
                string_list.append(&format!("{} ({})", lang.display_name, lang.native_name));
                ids.push(lang.id.clone());
            }
        }

        (string_list, ids)
    }

    pub fn create_target_model(registry: &LanguageRegistry) -> (gtk4::StringList, Vec<LanguageId>) {
        let string_list = gtk4::StringList::new(&[]);
        let mut ids = Vec::new();

        for lang in registry.list() {
            if lang.translation_supported && lang.tts_supported {
                string_list.append(&format!("{} ({})", lang.display_name, lang.native_name));
                ids.push(lang.id.clone());
            }
        }

        (string_list, ids)
    }
}
