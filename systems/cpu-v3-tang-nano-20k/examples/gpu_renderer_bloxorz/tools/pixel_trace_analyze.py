"""Exact ray/plane comparison for one opaque scene frame's observed covers.

The source vertices are the actual transformed Q16 positions. Positive determinant
means front only for consistently inward-wound application solids; legacy mixed
winding must be diagnosed separately. No transport/shader arithmetic is reused.
"""
import argparse
from collections import defaultdict
from fractions import Fraction
import json
from pathlib import Path


def cross(a, b):
    return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def exact(row):
    if row.get('global_depth_shift', 0):
        raise ValueError('Historical selectable-depth records require their frozen analyzer')
    p=row['position_q16']
    x,y=row['xy']
    ray=[2*x-399,2*y-239,512]
    normals=[cross(p[1],p[2]),cross(p[2],p[0]),cross(p[0],p[1])]
    determinant=dot(p[0],normals[0])
    sign=1 if determinant>0 else -1
    h=[dot(n,ray)*sign for n in normals]
    denominator=sum(h)
    if not determinant or denominator<=0:
        return None
    # Exact top-left coverage, before all N27 and N18/C28 compression.
    taus=[int(not (n[0]*sign>0 or n[0]==0 and n[1]*sign>0)) for n in normals]
    z_q16=Fraction(abs(determinant)*512,denominator)
    result=dict(row)
    result.update(exact_z=float(z_q16/65536),
                  exact_depth_code=float(z_q16/256),
                  exact_front=determinant>0,
                  exact_covered=all(value>=tau for value,tau in zip(h,taus)))
    return result,z_q16


def analyze(path, minimum):
    groups=defaultdict(list)
    for line in path.open(encoding='utf-8'):
        row=json.loads(line)
        # Opaque depth-writing surfaces only; shadows and UI are not a depth oracle.
        if not row['render']['control']&8:
            continue
        value=exact(row)
        if value:
            groups[(tuple(row['xy']),tuple(row['target_byte']))].append(value)
    wrong=[]
    for (xy,target),values in groups.items():
        written=[value for value in values if value[0]['depth_passing']]
        front=[value for value in values if value[0]['exact_front'] and value[0]['exact_covered']]
        if not written or not front:
            continue
        actual=written[-1]
        nearest=min(front,key=lambda value:value[1])
        separation=(actual[1]-nearest[1])/65536
        if separation>0:
            wrong.append(dict(xy=xy,target_byte=target,separation_world=float(separation),
                              above_report_threshold=separation>minimum,
                              actual=actual[0],nearest=nearest[0]))
    return dict(input=str(path),criterion='one opaque scene frame; exact top-left coverage and positive determinant',
                report_threshold_world=str(minimum),threshold_is_not_a_visual_pass_criterion=True,
                covered_target_pixels=len(groups),wrong_nearest_count=len(wrong),
                wrong_nearest_above_threshold=sum(row['above_report_threshold'] for row in wrong),
                wrong_nearest=wrong)


if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('covers',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--minimum-separation',type=Fraction,default=Fraction('0.005'))
    args=parser.parse_args()
    report=analyze(args.covers,args.minimum_separation)
    with args.output.open('x',encoding='utf-8') as out:
        json.dump(report,out,indent=2);out.write('\n')
    print(json.dumps({key:value for key,value in report.items() if key!='wrong_nearest'}))
