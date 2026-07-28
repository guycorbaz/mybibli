"""
Mock metadata server for E2E tests.
Returns deterministic BnF SRU XML, Google Books JSON, and Open Library JSON
responses for known ISBNs.
"""

import http.server
import json

# #427 — 100x150 red JPEG (quality 80), pregenerated with PIL, always valid.
TEST_COVER_JPEG_B64 = (
    "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAYEBQYFBAYGBQYHBwYIChAKCgkJChQODwwQFxQYGBcUFhYaHSUfGhsjHBYWICwgIyYnKSopGR8tMC0oMCUoKSj/2wBDAQcHBwoIChMKChMoGhYaKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCj/wAARCACWAGQDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDk6KKK8I/VgooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigD//Z"
)
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
        "thumbnail": "http://mock-metadata:9090/test-cover.jpg",
    },
    "9780201633610": {
        "title": "Design Patterns",
        "subtitle": "Elements of Reusable Object-Oriented Software",
        "description": "Classic software design patterns reference.",
        "authors": ["Erich Gamma", "Richard Helm"],
        "publisher": "Addison-Wesley Professional",
        "publishedDate": "1994-10-31",
        "pageCount": 395,
        "language": "en",
        "thumbnail": "http://mock-metadata:9090/test-cover.jpg",
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

        # --- Library of Congress SRU (MARC 21) — #439 ---
        elif path == "/LCDB":
            self._handle_loc_sru(params)

        # --- Library of Congress flat JSON search — #439 ---
        # Must precede the Google Books arm: "/books/" vs "/books/v1/volumes".
        elif path == "/books/":
            self._handle_loc_json(params)

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

        # --- BnF Couvertures endpoint (#427) ---
        elif path == "/couverture/image/image/recupererImage":
            self._handle_bnf_cover(params)

        # --- Inventaire.io entities endpoint (#427) ---
        elif path == "/api/entities/by-uris":
            self._handle_inventaire(params)

        # --- Inventaire.io image host (#427) ---
        elif path.startswith("/img/entities/"):
            self._handle_test_cover()

        else:
            self.send_response(404)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"Not Found")

    # --- #427 cover-fallback fixtures -------------------------------
    # cover-fallbacks.spec.ts scans these two spec-unique ISBNs; the BnF
    # SRU catch-all resolves their metadata (without a cover URL), then
    # the cover-resolution fallbacks find an image here:
    #   - 9786678000016 (specIsbn("BN", 1)) → BnF Couvertures serves it
    #   - 9787386000015 (specIsbn("IV", 1)) → Inventaire serves it
    BNF_COVER_EANS = {"9786678000016"}
    INVENTAIRE_COVER_ISBNS = {"9787386000015"}

    def _handle_bnf_cover(self, params):
        """#427 — BnF Service Couvertures. Mirrors the REAL API's quirk:
        'no cover' is an HTTP 500 with an HTML body, NOT a 404."""
        ean = params.get("EAN", [""])[0]
        if ean in self.BNF_COVER_EANS:
            self._handle_test_cover()
        else:
            body = b"<html><body>Erreur interne</body></html>"
            self.send_response(500)
            self.send_header("Content-Type", "text/html;charset=utf-8")
            self.end_headers()
            self.wfile.write(body)

    def _handle_inventaire(self, params):
        """#427 — Inventaire.io by-uris. Exercises the redirect shape
        (isbn: uri -> internal inv: uri), the common prod case."""
        uris = params.get("uris", [""])[0]
        isbn = uris.replace("isbn:", "").strip()
        if isbn in self.INVENTAIRE_COVER_ISBNS:
            response = {
                "entities": {
                    "inv:mock427entity": {
                        "claims": {"invp:P2": ["mock427coverhash"]}
                    }
                },
                "redirects": {f"isbn:{isbn}": "inv:mock427entity"},
            }
        else:
            response = {"entities": {}, "redirects": {}, "notFound": [uris]}
        body = json.dumps(response)
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def _handle_bnf(self, params):
        query = params.get("query", [""])[0]
        isbn = None
        if "adj" in query:
            parts = query.split('"')
            if len(parts) >= 2:
                isbn = parts[1].strip()

        # Blocklist: ISBNs that must return "not found" from BnF
        # - 9780000000002: used by provider-chain to test "all providers fail"
        # - 9780000000019: used by title-edit-no-metadata (#203) — title is created
        #   with empty metadata, exercising the edit-then-save flow
        # - Google Books known ISBNs: must NOT resolve via BnF so the chain falls through to Google Books
        NO_METADATA_ISBNS = {"9780000000002", "9780000000019", "9780134685991", "9780201633610"}

        if isbn and isbn in BNF_KNOWN_ISBNS:
            body = make_sru_response(BNF_KNOWN_ISBNS[isbn])
        elif isbn and isbn not in NO_METADATA_ISBNS:
            # Catch-all: return synthetic metadata for any unknown ISBN
            # (supports per-spec unique ISBN generation in E2E tests)
            body = make_sru_response({
                "title": f"Test Title {isbn}",
                "subtitle": "",
                "author_surname": "TestAuthor",
                "author_forename": "Synthetic",
                "publisher": "Test Publisher",
                "date": "2024",
                "language": "fre",
            })
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

    # --- Library of Congress mocks (#439) ---------------------------
    # catalog-marc-zones.spec.ts scans this spec-unique ISBN. The BnF
    # catch-all resolves it first (it answers for any ISBN) and supplies
    # 200$f but no edition statement and no note — so the chain's
    # zone-completion pass must reach LoC for those. That is exactly the
    # cross-provider path the spec exercises.
    LOC_MARC_ISBN = "9780449000014"

    def _handle_loc_json(self, params):
        """Flat ?fo=json search. Carries the cover URL; no MARC zones."""
        q = params.get("q", [""])[0]
        if q == self.LOC_MARC_ISBN:
            body = json.dumps({"results": [{
                "title": "Zone Completion Sample",
                "contributor": ["Sample Author"],
                "date": "2020",
                "language": ["english"],
            }]})
        else:
            body = json.dumps({"results": []})
        self.send_response(200)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def _handle_loc_sru(self, params):
        """SRU searchRetrieve returning MARC 21. Supplies 250$a and 500$a."""
        query = params.get("query", [""])[0]
        isbn = query.replace("bath.isbn=", "")
        if isbn == self.LOC_MARC_ISBN:
            body = """<?xml version="1.0"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:numberOfRecords>1</zs:numberOfRecords><zs:records><zs:record><zs:recordData><record xmlns="http://www.loc.gov/MARC21/slim">
  <datafield tag="245" ind1="1" ind2="0">
    <subfield code="a">Zone Completion Sample /</subfield>
    <subfield code="c">Sample Author.</subfield>
  </datafield>
  <datafield tag="250" ind1=" " ind2=" ">
    <subfield code="a">Congress Third edition.</subfield>
  </datafield>
  <datafield tag="500" ind1=" " ind2=" ">
    <subfield code="a">Congress general note.</subfield>
  </datafield>
</record></zs:recordData></zs:record></zs:records></zs:searchRetrieveResponse>"""
        else:
            body = """<?xml version="1.0"?>
<zs:searchRetrieveResponse xmlns:zs="http://www.loc.gov/zing/srw/"><zs:numberOfRecords>0</zs:numberOfRecords><zs:records/></zs:searchRetrieveResponse>"""
        self.send_response(200)
        self.send_header("Content-Type", "application/xml; charset=utf-8")
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
        """Serve a small 100x150 red JPEG test image.

        #427: the image is a pregenerated, embedded, VALID JPEG. The
        previous implementation depended on PIL (absent from the mock
        container) and fell back to a truncated hand-rolled JPEG that
        the app's `image` crate rejected — undetected for months
        because the app's unconditional http->https rewrite meant no
        mock-served cover was ever actually downloaded. Both bugs are
        fixed together (see src/services/cover.rs #427).
        """
        import base64
        body = base64.b64decode(TEST_COVER_JPEG_B64)
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
