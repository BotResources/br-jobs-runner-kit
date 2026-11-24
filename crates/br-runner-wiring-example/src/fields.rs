use br_graph_contract::{Fields, Reason};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub lines: Vec<String>,
}

pub struct AddLine(pub String);

impl Fields for Report {
    type Update = AddLine;

    fn empty_update() -> AddLine {
        AddLine(String::new())
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: AddLine) -> Result<(), Reason> {
        if !update.0.is_empty() {
            self.lines.push(update.0);
        }
        Ok(())
    }
}
