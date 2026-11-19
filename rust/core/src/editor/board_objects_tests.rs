use super::*;

fn tap(editor: &mut Editor, point: Pt, time: u64) {
    editor.pointer_down(0, point, time);
    editor.pointer_up(0, point, time + 1, false);
}

fn drag(editor: &mut Editor, start: Pt, finish: Pt, time: u64) {
    editor.pointer_down(0, start, time);
    editor.pointer_move(0, finish, time + 40);
    editor.pointer_up(0, finish, time + 60, false);
}

#[test]
fn table_select_moves_resizes_and_edits_a_cell() {
    let mut editor = Editor::new();
    editor.resize(960, 640, 1.0);
    editor.insert_table(3, 3);
    let shape = editor.doc.shapes()[0].clone();
    let id = &shape.id;
    let center = shape.bounds().center();
    assert_eq!(editor.doc.hit_test(center, 0.0).unwrap().id, *id);

    tap(&mut editor, center, 100);
    tap(&mut editor, center + Pt::new(144.0, 0.0), 190);
    assert!(editor.take_text_request().is_none());
    tap(&mut editor, center, 280);
    tap(&mut editor, center, 370);
    let request = editor.take_text_request().unwrap();
    let cell = request.cell.unwrap();
    assert_eq!(request.id, *id);
    assert_eq!(cell, Cell { row: 1, column: 1 });
    editor.commit_cell_text(id, cell, "Planning");

    let delta = Pt::new(45.0, 32.0);
    drag(&mut editor, center, center + delta, 500);
    let moved = editor.doc.get(id).unwrap().bounds();
    assert_eq!(moved, shape.bounds().shift(delta));
    let handle = moved.inflate(6.0).bottom_right();
    drag(&mut editor, handle, handle + Pt::new(90.0, 45.0), 700);
    let resized = editor.doc.get(id).unwrap().bounds();
    assert!(resized.width() > moved.width() && resized.height() > moved.height());

    editor.resize_table_grid(1, 1);
    editor.commit_cell_text(id, cell, "");
    editor.undo();
    editor.undo();
    let original_grid = editor.doc.get(id).unwrap().table.as_deref().unwrap();
    assert_eq!((original_grid.rows(), original_grid.columns()), (3, 3));
    assert_eq!(original_grid.text(cell), Some("Planning"));
    editor.redo();
    let larger_grid = editor.doc.get(id).unwrap().table.as_deref().unwrap();
    assert_eq!((larger_grid.rows(), larger_grid.columns()), (4, 4));
    assert_eq!(larger_grid.text(cell), Some("Planning"));
    editor.redo();
    assert_eq!(editor.doc.get(id).unwrap().table.as_deref().unwrap().text(cell), Some(""));
}

#[test]
fn handwriting_inside_one_table_cell_is_replaced_in_one_history_step() {
    let mut editor = Editor::new();
    editor.resize(960, 640, 1.0);
    editor.insert_table(3, 3);
    let table = editor.doc.shapes()[0].clone();
    let center = table.bounds().center();
    let ink = Shape::new("ink", Tool::Pen, vec![center, center + Pt::new(15.0, 12.0)]);
    editor.apply_remote(vec![ink], vec![]);
    editor.doc.select(Some("ink"));
    editor.convert_selection();
    let request = editor.take_ink_request().unwrap();
    editor.commit_ink(&request.ids, "Tuesday");

    let cell = Cell { row: 1, column: 1 };
    assert!(!editor.doc.contains("ink"));
    assert_eq!(editor.doc.get(&table.id).unwrap().table.as_deref().unwrap().text(cell), Some("Tuesday"));
    editor.undo();
    assert!(editor.doc.contains("ink"));
    assert_eq!(editor.doc.get(&table.id), Some(&table));

    editor.set_tool(Tool::Eraser, PenMode::Ink);
    tap(&mut editor, center, 900);
    assert!(!editor.doc.contains("ink"));
    assert!(editor.doc.contains(&table.id));
    tap(&mut editor, table.bounds().top_left(), 1100);
    assert!(!editor.doc.contains(&table.id));
}

#[test]
fn mind_map_moves_and_deletes_only_the_selected_branch() {
    let mut editor = Editor::new();
    editor.resize(1500, 800, 1.0);
    editor.set_grid(false);
    editor.insert_mind_map();
    editor.add_mind_topic(false);
    editor.add_mind_topic(false);
    let root = editor.doc.shapes()[0].clone();
    let child = editor.doc.shapes()[1].clone();
    let grandchild = editor.doc.shapes()[2].clone();
    editor.doc.select(Some(&child.id));
    editor.add_mind_topic(true);
    let sibling = editor.doc.shapes()[3].clone();
    assert_eq!(sibling.parent_id.as_deref(), Some(root.id.as_str()));

    let pixel = |layer: &Layer, point: Pt| {
        let i = (point.y as usize * layer.w as usize + point.x as usize) * 4;
        [layer.data[i], layer.data[i + 1], layer.data[i + 2], layer.data[i + 3]]
    };
    let midpoint = |parent: Rect, child: Rect| {
        let (a, b) = mindmap::endpoints(parent, child);
        (a + b) / 2.0
    };
    let old_edge = midpoint(root.bounds(), child.bounds());
    let [a, r, g, b] = render::BACKGROUND.to_be_bytes();
    let background = [r, g, b, a];
    editor.render();
    assert_ne!(pixel(&editor.scene, old_edge), background);

    let start = child.bounds().center();
    let shift = Pt::new(0.0, -160.0);
    let new_edge = midpoint(root.bounds(), child.bounds().shift(shift));
    editor.pointer_down(0, start, 1000);
    editor.pointer_move(0, start + shift, 1050);
    editor.render();
    assert_ne!(pixel(editor.frame(), new_edge), background);
    editor.pointer_up(0, start + shift, 1100, false);
    editor.render();
    assert_eq!(pixel(&editor.scene, old_edge), background);
    assert_ne!(pixel(&editor.scene, new_edge), background);
    assert_eq!(editor.doc.get(&child.id).unwrap().bounds(), child.bounds().shift(shift));
    assert_eq!(editor.doc.get(&grandchild.id).unwrap().bounds(), grandchild.bounds().shift(shift));
    assert_eq!(editor.doc.get(&sibling.id), Some(&sibling));

    tap(&mut editor, start + shift, 1300);
    tap(&mut editor, start + shift, 1400);
    let text = editor.take_text_request().unwrap();
    assert_eq!(text.id, child.id);
    editor.commit_text(&text.id, text.x, text.y, "Idea");
    assert_eq!(editor.doc.get(&child.id).unwrap().text, "Idea");
    editor.delete_selection();
    assert_eq!(editor.doc.shapes(), &[root, sibling]);
    editor.undo();
    assert_eq!(editor.doc.get(&grandchild.id).unwrap().parent_id.as_deref(), Some(child.id.as_str()));
    editor.redo();
    assert!(!editor.doc.contains(&grandchild.id));
}

#[test]
fn refining_handwriting_keeps_it_ink_selected_and_undoable() {
    let mut editor = Editor::new();
    editor.resize(960, 640, 1.0);
    // Three wavy "words" drifting up and down along a line.
    let word = |id: &str, x0: f64, y0: f64| {
        let points = (0..60)
            .map(|i| {
                let t = i as f64;
                Pt::new(x0 + t * 1.5, y0 - 20.0 * (t * 0.5).sin().abs())
            })
            .collect();
        Shape::new(id, Tool::Pen, points)
    };
    let before = vec![word("a", 100.0, 300.0), word("b", 210.0, 308.0), word("c", 320.0, 293.0)];
    editor.apply_remote(before.clone(), vec![]);
    editor.doc.select_many(["a", "b", "c"].map(String::from));
    editor.refine_selection();

    let bottom = |id: &str| editor.doc.get(id).unwrap().bounds().b;
    let spread = |b: &[f64]| b.iter().cloned().fold(f64::MIN, f64::max) - b.iter().cloned().fold(f64::MAX, f64::min);
    assert!(spread(&[bottom("a"), bottom("b"), bottom("c")]) < 4.0);
    for id in ["a", "b", "c"] {
        assert_eq!(editor.doc.get(id).unwrap().tool, Tool::Pen);
        assert!(editor.doc.is_selected(id));
    }
    editor.undo();
    for shape in &before {
        assert_eq!(editor.doc.get(&shape.id).map(|s| s.points.clone()), Some(shape.points.clone()));
    }
}
