use ace_tool::config::{Config, ConfigOptions};
use ace_tool::tools::search_context::{SearchContextArgs, SearchContextTool};

#[tokio::test]
#[ignore]
async fn test_real_search_context() {
    let token = "63d161ed58a6c90f774e9cb8b95d6041a984059e3800fffa50436dc4aa98e233".to_string();
    let base_url = "https://d6.api.augmentcode.com".to_string();
    
    let config = Config::new(base_url, token, ConfigOptions::default()).unwrap();
    let tool = SearchContextTool::new(config);
    
    let args = SearchContextArgs {
        project_root_path: Some("f:/claude-tools/ace-tool-rs".to_string()),
        query: Some("Where is USER_AGENT defined?".to_string()),
    };
    
    let result = tool.execute(args).await;
    println!("API response result: {}", result.text);
    
    if result.text.contains("Too Many Requests") || result.text.contains("Too many requests") {
        panic!("Rate limit occurred! Response: {}", result.text);
    }
    if result.text.contains("Error: Search failed") {
        panic!("Error occurred: {}", result.text);
    }
    
    println!("SUCCESS! Codebase retrieval returned results without rate limiting!");
}
