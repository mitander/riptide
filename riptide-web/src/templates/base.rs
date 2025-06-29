//! Base HTML template with navigation and common styles

/// Generates the base HTML template with navigation and CSS
pub fn base_template(title: &str, active_page: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>{title} - Riptide</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; 
               background: #0a0a0a; color: #ffffff; line-height: 1.6; }}
        
        /* Navigation */
        nav {{ background: #1a1a1a; border-bottom: 1px solid #333; padding: 0 20px; }}
        .nav-container {{ max-width: 1200px; margin: 0 auto; display: flex; align-items: center; height: 60px; }}
        .logo {{ font-size: 24px; font-weight: bold; color: #4a9eff; margin-right: 40px; }}
        .nav-links {{ display: flex; gap: 30px; flex: 1; }}
        .nav-links a {{ color: #ccc; text-decoration: none; padding: 8px 16px; border-radius: 6px; 
                        transition: all 0.2s; }}
        .nav-links a:hover {{ color: #4a9eff; background: #2a2a2a; }}
        .nav-links a.active {{ color: #4a9eff; background: #2a4a6a; }}
        
        /* Main Content */
        .container {{ max-width: 1200px; margin: 0 auto; padding: 30px 20px; }}
        .page-header {{ margin-bottom: 30px; }}
        .page-header h1 {{ font-size: 32px; margin-bottom: 10px; }}
        .page-header p {{ color: #aaa; font-size: 16px; }}
        
        /* Cards */
        .card {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 20px; margin-bottom: 20px; }}
        .card h3 {{ color: #4a9eff; margin-bottom: 15px; }}
        
        /* Stats Grid */
        .stats-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 30px; }}
        .stat-card {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 20px; text-align: center; }}
        .stat-value {{ font-size: 28px; font-weight: bold; color: #4a9eff; display: block; }}
        .stat-label {{ color: #aaa; margin-top: 5px; }}
        
        /* Forms */
        .search-form {{ margin-bottom: 30px; }}
        .search-form input {{ width: 100%; max-width: 400px; padding: 12px; border: 1px solid #333; 
                           background: #2a2a2a; color: #fff; border-radius: 6px; font-size: 16px; }}
        .search-form button {{ padding: 12px 24px; background: #4a9eff; color: #fff; border: none; 
                             border-radius: 6px; cursor: pointer; margin-left: 10px; font-size: 16px; }}
        .search-form button:hover {{ background: #3a8edf; }}
        
        /* Tables */
        .table {{ width: 100%; background: #1a1a1a; border: 1px solid #333; border-radius: 8px; overflow: hidden; }}
        .table th {{ background: #2a2a2a; padding: 12px; text-align: left; color: #4a9eff; font-weight: 600; }}
        .table td {{ padding: 12px; border-top: 1px solid #333; }}
        .table tr:hover {{ background: #222; }}
        
        /* Buttons */
        .btn {{ padding: 8px 16px; background: #4a9eff; color: #fff; border: none; border-radius: 6px; 
                cursor: pointer; text-decoration: none; display: inline-block; }}
        .btn:hover {{ background: #3a8edf; }}
        .btn-small {{ padding: 6px 12px; font-size: 14px; }}
        .btn:disabled {{ background: #666; cursor: not-allowed; }}
        
        /* Grid */
        .grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 20px; }}
        .grid-item {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 15px; }}
        .grid-item h4 {{ color: #4a9eff; margin-bottom: 10px; }}
        .grid-item p {{ color: #aaa; font-size: 14px; }}
        
        /* Loading */
        .loading {{ text-align: center; color: #aaa; padding: 40px; }}
    </style>
    
    <script>
        // Utility function for API calls
        async function apiCall(endpoint) {{
            try {{
                const response = await fetch(endpoint);
                if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
                return await response.json();
            }} catch (error) {{
                console.error('API call failed:', error);
                throw error;
            }}
        }}
        
        // Auto-refresh stats every 30 seconds
        setInterval(async () => {{
            try {{
                await apiCall('/api/stats');
            }} catch (error) {{
                console.error('Stats refresh failed:', error);
            }}
        }}, 30000);
    </script>
</head>
<body>
    <nav>
        <div class="nav-container">
            <div class="logo">Riptide</div>
            <div class="nav-links">
                <a href="/" class="{dashboard_active}">Dashboard</a>
                <a href="/search" class="{search_active}">Search</a>
                <a href="/torrents" class="{torrents_active}">Torrents</a>
                <a href="/library" class="{library_active}">Library</a>
            </div>
        </div>
    </nav>
    
    <div class="container">
        {content}
    </div>
</body>
</html>"#,
        title = title,
        content = content,
        dashboard_active = if active_page == "dashboard" {
            "active"
        } else {
            ""
        },
        search_active = if active_page == "search" {
            "active"
        } else {
            ""
        },
        torrents_active = if active_page == "torrents" {
            "active"
        } else {
            ""
        },
        library_active = if active_page == "library" {
            "active"
        } else {
            ""
        }
    )
}
