#!/usr/bin/env python3
import os
import sys
import urllib.parse

print("Content-Type: text/html")
print()  # ligne vide obligatoire

print("""<html>
<body>""")

print("<h1>CGI Test OK ✅</h1>")

print("<h2>Request Method</h2>")
print(os.environ.get("REQUEST_METHOD", "UNKNOWN"))

print("<h2>Query String</h2>")
print(os.environ.get("QUERY_STRING", ""))

if os.environ.get("REQUEST_METHOD") == "POST":
    length = int(os.environ.get("CONTENT_LENGTH", 0))
    body = sys.stdin.read(length)
    print("<h2>POST Data</h2>")
    print(body)

print("<h2>CGI Environment</h2>")
print("<pre>")
for k, v in os.environ.items():
    print(f"{k}={v}")
print("</pre>")

print("""
</body>
</html>
""")
