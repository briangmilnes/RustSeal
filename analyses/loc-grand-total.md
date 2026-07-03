<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 4px 8px; vertical-align: top; }
thead th { position: sticky; top: 0; background: #fff; z-index: 2; box-shadow: 0 1px 0 #bbb; }
thead th:first-child, tbody td:first-child { position: sticky; left: 0; background: #fff; z-index: 1; }
thead th:first-child { z-index: 3; }
</style>
# csts-analyze-loc — directory `/home/milnes/projects/RustSeal/projects` — 

Parse-based lines-of-code — `code`/`comment`/`blank` from the vcst CST trivia, never `wc -l`/regex (rule 1.5).

Columns: `segment` = the scanned sub-tree; `total` = `code` + `comment` + `blank`; `files` = distinct `.rs` files parsed (`target/` skipped).

| # | segment | total | code | comment | blank | files |
|---|---------|------:|-----:|--------:|------:|------:|
| 1 | `.` | 54863822 | 44211859 | 7835442 | 2816521 | 297422 |
| — | **TOTALS** | **54863822** | **44211859** | **7835442** | **2816521** | **297422** |
