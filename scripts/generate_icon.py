"""Generate the code-native timeline application mark and Windows ICO container."""
from pathlib import Path
import struct
import zlib
root=Path(__file__).resolve().parents[1]/"assets"
root.mkdir(exist_ok=True)
w=h=64
pixels=bytearray([28,31,35,255]*(w*h))
def rect(x,y,width,height,color):
    for j in range(y,y+height):
        for i in range(x,x+width):
            offset=(j*w+i)*4
            pixels[offset:offset+4]=bytes(color)
rect(7,10,50,3,(132,143,152,255))
for y in (21,34,47):
    rect(7,y,50,9,(44,49,55,255))
rect(7,21,19,9,(98,146,181,255));rect(28,21,29,9,(98,146,181,255))
rect(14,34,35,9,(98,155,132,255));rect(23,47,29,9,(98,155,132,255))
rect(31,15,2,44,(235,179,86,255))
def chunk(kind,data):
    return struct.pack(">I",len(data))+kind+data+struct.pack(">I",zlib.crc32(kind+data)&0xffffffff)
raw=b"".join(b"\x00"+pixels[y*w*4:(y+1)*w*4] for y in range(h))
png=b"\x89PNG\r\n\x1a\n"+chunk(b"IHDR",struct.pack(">IIBBBBB",w,h,8,6,0,0,0))+chunk(b"IDAT",zlib.compress(raw,9))+chunk(b"IEND",b"")
(root/"svbb.png").write_bytes(png)
(root/"svbb.ico").write_bytes(struct.pack("<HHH",0,1,1)+struct.pack("<BBBBHHII",w,h,0,0,1,32,len(png),22)+png)
