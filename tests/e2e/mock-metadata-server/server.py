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
        "thumbnail": "http://localhost:9090/test-cover.jpg",
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

        # --- OMDb endpoint (must check before BnF since both use /) ---
        if (path == "/" or path == "") and "apikey" in params:
            self._handle_omdb(parsed.query)

        # --- BnF SRU endpoint ---
        elif path == "/" or path == "" or "SRU" in path:
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

        # --- MusicBrainz endpoint ---
        elif path.startswith("/ws/2/release/"):
            self._handle_musicbrainz(parsed.query)

        # --- TMDb endpoint ---
        elif path.startswith("/3/search/movie"):
            self._handle_tmdb(parsed.query)

        # --- Test cover image ---
        elif path == "/test-cover.jpg":
            self._handle_test_cover()

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

    # --- MusicBrainz mock ---
    def _handle_musicbrainz(self, query_string):
        params = urllib.parse.parse_qs(query_string)
        query = params.get("query", [""])[0]
        upc = query.replace("barcode:", "")

        if upc == "0093624738626":
            body = json.dumps({"releases": [{
                "id": "b5748ac0-test-mock-abcd-ef1234567890",
                "title": "OK Computer",
                "date": "1997-06-16",
                "disambiguation": "reissue",
                "track-count": 12,
                "artist-credit": [{"name": "Radiohead"}],
                "label-info": [{"label": {"name": "Parlophone"}}]
            }]})
            self.send_response(200)
        else:
            body = json.dumps({"releases": []})
            self.send_response(200)

        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    # --- OMDb mock ---
    def _handle_omdb(self, query_string):
        params = urllib.parse.parse_qs(query_string)

        if "i" in params:
            # Detail request by imdbID
            imdb_id = params["i"][0]
            if imdb_id == "tt0137523":
                body = json.dumps({
                    "Title": "Fight Club", "Year": "1999",
                    "Director": "David Fincher",
                    "Plot": "An insomniac office worker forms an underground fight club.",
                    "Poster": "https://example.com/fightclub.jpg",
                    "Runtime": "139 min", "Response": "True"
                })
            else:
                body = json.dumps({"Response": "False", "Error": "Movie not found!"})
            self.send_response(200)
        elif "s" in params:
            # Search request
            search = params["s"][0]
            if search == "5051889004578":
                body = json.dumps({"Search": [
                    {"Title": "Fight Club", "Year": "1999", "imdbID": "tt0137523", "Type": "movie"}
                ], "totalResults": "1", "Response": "True"})
            else:
                body = json.dumps({"Response": "False", "Error": "Movie not found!"})
            self.send_response(200)
        else:
            body = json.dumps({"Response": "False", "Error": "Invalid request"})
            self.send_response(400)

        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    # --- TMDb mock ---
    def _handle_tmdb(self, query_string):
        params = urllib.parse.parse_qs(query_string)
        query = params.get("query", [""])[0]

        if query == "5051889004578":
            body = json.dumps({"results": [{
                "title": "Fight Club",
                "overview": "An insomniac office worker forms an underground fight club.",
                "release_date": "1999-10-15",
                "poster_path": "/pB8BM7pdSp6B6Ih7QZ4DrQ3PmJK.jpg",
                "original_language": "en"
            }], "total_results": 1})
        else:
            body = json.dumps({"results": [], "total_results": 0})
        self.send_response(200)

        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def _handle_test_cover(self):
        """Serve a small 100x150 red JPEG test image."""
        import struct
        # Minimal valid JPEG: 2x2 red pixels (smallest valid JPEG)
        # Using PIL if available, otherwise return a minimal JPEG
        try:
            from PIL import Image
            import io
            img = Image.new("RGB", (100, 150), color=(200, 50, 50))
            buf = io.BytesIO()
            img.save(buf, format="JPEG", quality=80)
            body = buf.getvalue()
        except ImportError:
            # Fallback: hardcoded minimal 1x1 JPEG
            body = bytes([
                0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
                0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43,
                0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09,
                0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12,
                0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
                0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29,
                0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
                0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01,
                0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00,
                0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
                0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03,
                0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7D,
                0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
                0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
                0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72,
                0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28,
                0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7B, 0x94,
                0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xD9,
            ])
        self.send_response(200)
        self.send_header("Content-Type", "image/jpeg")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        print(f"[mock-metadata] {format % args}")


if __name__ == "__main__":
    port = 9090
    server = http.server.HTTPServer(("0.0.0.0", port), MockMetadataHandler)
    print(f"Mock metadata server running on port {port}")
    print(f"  BnF ISBNs: {list(BNF_KNOWN_ISBNS.keys())}")
    print(f"  Google Books ISBNs: {list(GOOGLE_BOOKS_KNOWN_ISBNS.keys())}")
    print(f"  Open Library ISBNs: {list(OPEN_LIBRARY_KNOWN_ISBNS.keys())}")
    print(f"  Test UPCs: CD=0093624738626, DVD=5051889004578")
    server.serve_forever()
