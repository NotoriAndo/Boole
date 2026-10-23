"""Independent stdlib SHA-256/RLP vector for the devnet M2ROOT deposit."""
from pathlib import Path
import hashlib
import json

ROOT = Path(__file__).resolve().parents[2]
def rlp(data):
    if isinstance(data, list):
        body = b"".join(map(rlp, data))
        offset = 0xc0
    else:
        body = data
        if len(body) == 1 and body[0] < 0x80:
            return body
        offset = 0x80
    if len(body) < 56:
        return bytes([offset + len(body)]) + body
    length = len(body).to_bytes((len(body).bit_length() + 7) // 8, "big")
    return bytes([offset + 55 + len(length)]) + length + body

origin = bytes([3]) * 32
number = 7
root = bytes([19]) * 32
source = hashlib.sha256(b"BOOLE-S1/M2ROOT" + origin + bytes(32)).digest()
data = b"m2ro" + number.to_bytes(8, "big") + root
fields = [source, bytes.fromhex("deaddeaddeaddeaddeaddeaddeaddeaddead0001"), bytes.fromhex("42000000000000000000000000000000000000b2"), b"", b"", (1_000_000).to_bytes(3, "big"), b"", data]
vector = {"schema":"boole-devnet-m2root-vector-v1.2", "origin":"0x"+origin.hex(), "number":number, "root":"0x"+root.hex(), "source_hash":"0x"+source.hex(), "gas":1_000_000, "data":"0x"+data.hex(), "deposit":"0x"+(b"\x7e"+rlp(fields)).hex()}
(ROOT / "fixtures/devnet-m2/root-system-v1.2.json").write_text(json.dumps(vector, indent=2)+"\n")
