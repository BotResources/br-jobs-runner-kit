use std::collections::BTreeMap;

use crate::cluster::WriteError;

use super::Engine;

#[derive(Default)]
pub(crate) struct Refusals {
    held: BTreeMap<(String, &'static str, String), String>,
}

impl Refusals {
    pub(crate) fn refuse(&mut self, namespace: &str, call: &'static str, subject: &str, why: &str) {
        self.held.insert(
            (namespace.to_owned(), call, subject.to_owned()),
            why.to_owned(),
        );
    }

    pub(crate) fn clear(&mut self, namespace: &str, call: &'static str, subject: &str) {
        self.held
            .remove(&(namespace.to_owned(), call, subject.to_owned()));
    }

    pub(crate) fn naming(&self, namespace: &str) -> Option<String> {
        let mut by_call: BTreeMap<&'static str, (usize, &str)> = BTreeMap::new();
        for ((held_in, call, _), why) in &self.held {
            if held_in == namespace {
                by_call
                    .entry(call)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, why));
            }
        }
        if by_call.is_empty() {
            return None;
        }
        let named: Vec<String> = by_call
            .into_iter()
            .map(|(call, (count, why))| match count {
                1 => format!("{call}: {why}"),
                many => format!("{call} for {many} objects: {why}"),
            })
            .collect();
        Some(format!("refused by the API server: {}", named.join("; ")))
    }
}

impl Engine {
    pub(super) fn note_write<T>(
        &mut self,
        namespace: &str,
        call: &'static str,
        subject: &str,
        result: &Result<T, WriteError>,
    ) {
        match result {
            Ok(_) => self.refusals.clear(namespace, call, subject),
            Err(WriteError::Refused { reason, .. }) => {
                self.refusals.refuse(namespace, call, subject, reason);
            }
            Err(_) => {}
        }
    }
}
