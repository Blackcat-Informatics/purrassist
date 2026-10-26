// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use std::{env, fs};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--declarations") {
        let input = fs::read_to_string(&args[1])?;
        let tools = purrassist_core::tool_call::ToolSet::from_mcp(&input, purrassist_core::Budget::default()).map_err(|e| format!("{e:?}"))?;
        for form in purrassist_core::tool_call::DeclarationForm::ALL {
            for name in ["query_customer_orders", "edit_file"] {
                let rendered = tools.render_declaration(name, form).map_err(|e| format!("{e:?}"))?;
                fs::write(format!("{}/{form:?}-{name}.txt", args[2]), rendered)?;
            }
        }
        return Ok(());
    }
    let mut count = 0;
    for path in env::args().skip(1) {
        let text = fs::read_to_string(&path)?;
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            if !line.starts_with("```") { continue; }
            let language = line.trim_start_matches('`');
            let mut body = String::new();
            for line in lines.by_ref() {
                if line == "```" { break; }
                body.push_str(line); body.push('\n');
            }
            match language {
                "turtle" => { purrdf_rdf::parse_dataset(body.as_bytes(), "text/turtle", None).map_err(|e| format!("{path} turtle: {e:?}"))?; }
                "srl" => { purrdf_shapes::srl::check(&body, None, &[], purrdf_shapes::srl::CheckLevel::Stratified).map_err(|e| format!("{path} srl: {e:?}"))?; }
                _ => continue,
            }
            count += 1;
            println!("{path}: {language} block {count} PASS");
        }
    }
    if count == 0 { return Err("no RDF/SRL blocks checked".into()); }
    Ok(())
}
