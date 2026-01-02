#!/usr/bin/env python3
import os
import sys

print("Content-Type: text/html\r")
print("\r")
print("<html><body>")
print("<h1>CGI Test - Python</h1>")
print(f"<p>Method: {os.environ.get('REQUEST_METHOD', 'N/A')}</p>")
print(f"<p>Query: {os.environ.get('QUERY_STRING', 'N/A')}</p>")
print(f"<p>Path: {os.environ.get('PATH_INFO', 'N/A')}</p>")

# Lire POST data si présent
if os.environ.get('REQUEST_METHOD') == 'POST':
    content_length = int(os.environ.get('CONTENT_LENGTH', 0))
    if content_length > 0:
        post_data = sys.stdin.read(content_length)
        print(f"<p>POST Data: {post_data}</p>")

print("</body></html>")