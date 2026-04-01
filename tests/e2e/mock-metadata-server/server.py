"""
Mock metadata server for E2E tests.
Returns deterministic BnF SRU XML responses for known ISBNs.
"""

import http.server
import urllib.parse

KNOWN_ISBNS = {
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


EMPTY_RESPONSE = """<?xml version="1.0" encoding="UTF-8"?>
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
  <srw:numberOfRecords>0</srw:numberOfRecords>
  <srw:records/>
</srw:searchRetrieveResponse>"""


class MockBnfHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        params = urllib.parse.parse_qs(parsed.query)

        query = params.get("query", [""])[0]

        # Extract ISBN from query like: bib.isbn adj "9782070360246"
        isbn = None
        if "adj" in query:
            parts = query.split('"')
            if len(parts) >= 2:
                isbn = parts[1].strip()

        if isbn and isbn in KNOWN_ISBNS:
            body = make_sru_response(KNOWN_ISBNS[isbn])
            self.send_response(200)
        else:
            body = EMPTY_RESPONSE
            self.send_response(200)

        self.send_header("Content-Type", "application/xml; charset=utf-8")
        self.end_headers()
        self.wfile.write(body.encode("utf-8"))

    def log_message(self, format, *args):
        print(f"[mock-bnf] {format % args}")


if __name__ == "__main__":
    port = 9090
    server = http.server.HTTPServer(("0.0.0.0", port), MockBnfHandler)
    print(f"Mock BnF metadata server running on port {port}")
    server.serve_forever()
