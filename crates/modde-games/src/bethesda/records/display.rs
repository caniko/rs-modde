use super::*;

impl fmt::Display for UnresolvedFormReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.expected_plugin {
            _ if self.mod_index_out_of_range => write!(
                f,
                "{} {} {:08X} references FormID {:08X} with a mod index beyond the master list via {}",
                self.source_plugin,
                self.source_record,
                self.source_form_id,
                self.target_form_id,
                self.subrecord
            ),
            Some(plugin) => write!(
                f,
                "{} {} {:08X} references unresolved {:08X} in {} via {}",
                self.source_plugin,
                self.source_record,
                self.source_form_id,
                self.target_form_id,
                plugin,
                self.subrecord
            ),
            None => write!(
                f,
                "{} {} {:08X} references unresolved {:08X} via {}",
                self.source_plugin,
                self.source_record,
                self.source_form_id,
                self.target_form_id,
                self.subrecord
            ),
        }
    }
}
