"""Wire layer — FlatBuffers codec and TCP framing.

Injects the generated/ directory onto sys.path so that the FlatBuffers
generated code's internal imports (e.g. ``from pyrat.protocol.X import X``)
resolve correctly.
"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "generated"))
