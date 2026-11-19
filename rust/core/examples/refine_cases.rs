//! Runs handwriting refine over test cases: `refine_cases in.json out.json`.
//! Each case is {"name", "messy": [[[x, y], ...], ...]}; the output adds "refined".
use canvas_core::geom::Pt;
use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let mut out = Vec::new();
    for mut c in cases {
        let strokes: Vec<Vec<Pt>> = c["messy"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_array().unwrap().iter().map(|p| Pt::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect())
            .collect();
        let t = std::time::Instant::now();
        let refined = canvas_core::refine::refine(&strokes);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        c["refined"] = json!(refined.iter().map(|s| s.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>());
        c["ms"] = json!(ms);
        out.push(c);
    }
    std::fs::write(&args[2], serde_json::to_string(&out).unwrap()).unwrap();
}
