#[cfg(test)]
mod tests {
    use loom_acp::content::extract_locations;
    use serde_json::json;
    
    #[test]
    fn test_extract_path_from_read() {
        let input = json!({
            "path": "/path/to/file.txt"
        });
        
        let locations = extract_locations("read", &input);
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, "/path/to/file.txt");
        assert_eq!(locations[0].line, None);
    }
    
    #[test]
    fn test_extract_multiple_paths_from_move() {
        let input = json!({
            "source": "/path/old.txt",
            "target": "/path/new.txt"
        });
        
        let locations = extract_locations("move_file", &input);
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].path, "/path/old.txt");
        assert_eq!(locations[1].path, "/path/new.txt");
    }
    
    #[test]
    fn test_extract_line_numbers() {
        let input = json!({
            "path": "/path/to/file.txt",
            "line": 42
        });
        
        let locations = extract_locations("read", &input);
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, "/path/to/file.txt");
        assert_eq!(locations[0].line, Some(42));
    }
    
    #[test]
    fn test_extract_from_grep() {
        let input = json!({
            "pattern": "test",
            "path": "/path/to/search"
        });
        
        let locations = extract_locations("grep", &input);
        assert!(locations.len() >= 1);
        assert_eq!(locations[0].path, "/path/to/search");
    }
    
    #[test]
    fn test_no_location_for_unknown_tool() {
        let input = json!({
            "param": "value"
        });
        
        let locations = extract_locations("unknown_tool", &input);
        assert_eq!(locations.len(), 0);
    }
    
    #[test]
    fn test_edit_with_path() {
        let input = json!({
            "path": "/path/to/file.txt",
            "old_string": "old",
            "new_string": "new"
        });
        
        let locations = extract_locations("edit", &input);
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, "/path/to/file.txt");
    }
    
    #[test]
    fn test_delete_file() {
        let input = json!({
            "path": "/path/to/delete.txt"
        });
        
        let locations = extract_locations("delete_file", &input);
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, "/path/to/delete.txt");
    }
}
