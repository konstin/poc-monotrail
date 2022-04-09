import sys

import inflection
import upsidedown

assert upsidedown.transform(inflection.titleize('hello world!')) == "¡pꞁɹoM oꞁꞁǝH"

if len(sys.argv) > 1:
    print(upsidedown.transform(inflection.titleize(sys.argv[1])))
