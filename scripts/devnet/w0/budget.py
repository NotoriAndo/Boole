"""One monotonic 33-minute budget, including staging, build, test and cleanup."""
import math,sys,time

def remaining(deadline,ceiling,reserve,now=None):
    now=time.monotonic() if now is None else now
    value=min(ceiling,math.floor(deadline-now-reserve))
    if not all(math.isfinite(x) for x in (deadline,ceiling,reserve,now)) or ceiling<=0 or reserve<0 or value<1:raise ValueError('whole-run budget exhausted')
    return value
if __name__=='__main__':print(remaining(*map(float,sys.argv[1:])))
