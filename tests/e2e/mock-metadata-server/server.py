"""
Mock metadata server for E2E tests.
Returns deterministic BnF SRU XML, Google Books JSON, and Open Library JSON
responses for known ISBNs.
"""

import http.server
import json
import urllib.parse

# --- BnF known ISBNs (returned by BnF endpoint) ---
BNF_KNOWN_ISBNS = {
    "9782070360246": {
        "title": "L'Étranger",
        "subtitle": "roman",
        "author_surname": "Camus",
        "author_forename": "Albert",
        "publisher": "Gallimard",
        "date": "1942",
        "language": "fre",
    },
    "9780306406157": {
        "title": "The Art of Electronics",
        "subtitle": "",
        "author_surname": "Horowitz",
        "author_forename": "Paul",
        "publisher": "Cambridge University Press",
        "date": "2015",
        "language": "eng",
    },
    "9791032305560": {
        "title": "Les Misérables",
        "subtitle": "roman",
        "author_surname": "Hugo",
        "author_forename": "Victor",
        "publisher": "Le Livre de Poche",
        "date": "2014",
        "language": "fre",
    },
}

# --- Google Books known ISBNs (only for ISBNs NOT in BnF, to test fallback) ---
GOOGLE_BOOKS_KNOWN_ISBNS = {
    "9780134685991": {
        "title": "Effective Java",
        "subtitle": "Third Edition",
        "description": "The definitive guide to Java platform best practices.",
        "authors": ["Joshua Bloch"],
        "publisher": "Addison-Wesley Professional",
        "publishedDate": "2018-01-06",
        "pageCount": 416,
        "language": "en",
        "thumbnail": "http://books.google.com/books/content?id=effectivejava",
    },
}

# --- Open Library known ISBNs (for ISBNs NOT in BnF or Google Books) ---
OPEN_LIBRARY_KNOWN_ISBNS = {
    "9780596007126": {
        "title": "Head First Design Patterns",
        "subtitle": "A Brain-Friendly Guide",
        "description": "Learning design patterns with visual, fun approach.",
        "authors": [{"key": "/authors/OL1234A"}],
        "publishers": ["O'Reilly Media"],
        "publish_date": "2004",
        "covers": [54321],
        "number_of_pages": 694,
    },
}

# Author data for Open Library author resolution
OPEN_LIBRARY_AUTHORS = {
    "/authors/OL1234A": {"name": "Eric Freeman"},
}


def make_sru_response(isbn_data):
    """Generate a BnF SRU XML response for a known ISBN."""
    subtitle_field = ""
    if isbn_data["subtitle"]:
        subtitle_field = f'<mxc:subfield code="e">{isbn_data["subtitle"]}</mxc:subfield>'

    return f"""<?xml version="1.0" encoding="UTF-8"?>
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
  <srw:numberOfRecords>1</srw:numberOfRecords>
  <srw:records>
    <srw:record>
      <srw:recordData>
        <mxc:record xmlns:mxc="info:lc/xmlns/marcxchange-v2">
          <mxc:datafield tag="101" ind1=" " ind2=" ">
            <mxc:subfield code="a">{isbn_data["language"]}</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="200" ind1="1" ind2=" ">
            <mxc:subfield code="a">{isbn_data["title"]}</mxc:subfield>
            {subtitle_field}
            <mxc:subfield code="f">{isbn_data["author_forename"]} {isbn_data["author_surname"]}</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="210" ind1=" " ind2=" ">
            <mxc:subfield code="c">{isbn_data["publisher"]}</mxc:subfield>
            <mxc:subfield code="d">{isbn_data["date"]}</mxc:subfield>
          </mxc:datafield>
          <mxc:datafield tag="700" ind1=" " ind2=" ">
            <mxc:subfield code="a">{isbn_data["author_surname"]}</mxc:subfield>
            <mxc:subfield code="b">{isbn_data["author_forename"]}</mxc:subfield>
          </mxc:datafield>
        </mxc:record>
      </srw:recordData>
    </srw:record>
  </srw:records>
</srw:searchRetrieveResponse>"""


EMPTY_SRU_RESPONSE = """<?xml version="1.0" encoding="UTF-8"?>
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
  <srw:numberOfRecords>0</srw:numberOfRecords>
  <srw:records/>
</srw:searchRetrieveResponse>"""


class MockMetadataHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path
        params = urllib.parse.parse_qs(parsed.query)

        # --- BnF SRU endpoint ---
        if path == "/" or path == "" or "SRU" in path:
            self._handle_bnf(params)

        # --- Google Books endpoint ---
        elif path == "/books/v1/volumes":
            self._handle_google_books(params)

        # --- Open Library ISBN endpoint ---
        elif path.startswith("/isbn/"):
            self._handle_open_library_isbn(path)

        # --- Open Library authors endpoint ---
        elif path.startswith("/authors/"):
            self._handle_open_library_author(path)

        else:
            self.send_response(404)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"Not Found")

    def _handle_bnf(self, params):
        query = params.get("query", [""])[0]
        isbn = None
        if "adj" in query:
            parts = query.split('"')
            if len(parts) >= 2:
                isbn = parts[1].strip()

        if isbn and isbn in BNF_KNOWN_ISBNS:
            body = make_sru_response(BNF_KNOWN_ISBNS[isbn])
        else:
            body = EMPTY_SRU_RESPONSE

        self.send_response(200)
        self.send_header("Content-Type", "application/xml; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def _handle_google_books(self, params):
        q = params.get("q", [""])[0]
        isbn = None
        if q.startswith("isbn:"):
            isbn = q[5:]

        if isbn and isbn in GOOGLE_BOOKS_KNOWN_ISBNS:
            data = GOOGLE_BOOKS_KNOWN_ISBNS[isbn]
            response = {
                "totalItems": 1,
                "items": [{
                    "volumeInfo": {
                        "title": data["title"],
                        "subtitle": data.get("subtitle"),
                        "description": data.get("description"),
                        "authors": data.get("authors", []),
                        "publisher": data.get("publisher"),
                        "publishedDate": data.get("publishedDate"),
                        "pageCount": data.get("pageCount"),
                        "language": data.get("language"),
                        "imageLinks": {
                            "thumbnail": data.get("thumbnail", "")
                        } if data.get("thumbnail") else {},
                    }
                }]
            }
        else:
            response = {"totalItems": 0}

        body = json.dumps(response)
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def _handle_open_library_isbn(self, path):
        # path like /isbn/9780596007126.json
        isbn = path.replace("/isbn/", "").replace(".json", "")

        if isbn in OPEN_LIBRARY_KNOWN_ISBNS:
            body = json.dumps(OPEN_LIBRARY_KNOWN_ISBNS[isbn])
            self.send_response(200)
        else:
            self.send_response(404)
            body = json.dumps({"error": "Not found"})

        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def _handle_open_library_author(self, path):
        # path like /authors/OL1234A.json
        key = path.replace(".json", "")  # /authors/OL1234A

        if key in OPEN_LIBRARY_AUTHORS:
            body = json.dumps(OPEN_LIBRARY_AUTHORS[key])
            self.send_response(200)
        else:
            self.send_response(404)
            body = json.dumps({"error": "Not found"})

        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def log_message(self, format, *args):
        print(f"[mock-metadata] {format % args}")


if __name__ == "__main__":
    port = 9090
    server = http.server.HTTPServer(("0.0.0.0", port), MockMetadataHandler)
    print(f"Mock metadata server running on port {port}")
    print(f"  BnF ISBNs: {list(BNF_KNOWN_ISBNS.keys())}")
    print(f"  Google Books ISBNs: {list(GOOGLE_BOOKS_KNOWN_ISBNS.keys())}")
    print(f"  Open Library ISBNs: {list(OPEN_LIBRARY_KNOWN_ISBNS.keys())}")
    server.serve_forever()
