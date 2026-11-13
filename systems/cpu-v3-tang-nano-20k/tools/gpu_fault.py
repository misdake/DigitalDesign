"""Host names for the existing first-fault protocol; unknown values stay raw."""
SITES = {1:'memory',2:'GPU initialization',3:'surface',4:'command recording',5:'display',6:'GPU execution',7:'scene object',8:'mesh',9:'texture view'}
CODES = {
 1:{1:'invalid physical extent'},
 2:{1:'context initialization failed',2:'command buffer creation failed'},
 3:{1:'allocation failed',2:'invalid surface extent',3:'release failed'},
 4:{1:'reset rejected',2:'target binding rejected',3:'pass operation rejected',4:'pass completion rejected',5:'copy or clear rejected',6:'command buffer ownership mismatch'},
 5:{1:'open rejected',2:'swap staging rejected',3:'swap wait or polling failed'},
 6:{1:'submission preparation rejected',2:'execution start rejected',3:'execution fault',4:'wait lifecycle mismatch',5:'capture arm rejected',6:'capture publication rejected',7:'capture action rejected'},
 7:{1:'object allocation failed',2:'invalid object description',3:'object release failed'},
 8:{1:'mesh allocation failed',2:'invalid native mesh',3:'invalid mesh range',4:'mesh release failed'},
 9:{1:'texture binding rejected'},
}


def decode(words):
    if len(words) != 16 or words[0] != 0xfa17:
        return None
    site, code = words[1:3]
    return dict(site=site, code=code, operation=SITES.get(site,'unknown site'),
                reason=CODES.get(site,{}).get(code,'unknown code'), detail0=words[3],detail1=words[4],
                dseg3=words[5],words=words)
