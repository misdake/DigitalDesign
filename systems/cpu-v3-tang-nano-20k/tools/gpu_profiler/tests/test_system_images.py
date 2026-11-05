"""Masked physical writes, target epochs, exact native planes and open capture."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
import system_images


class NativeImages(unittest.TestCase):
    def test_four_later_groups_keep_all_preceding_physical_writes(self):
        import hashlib
        initial=bytes(8388608);commands=[];writes=[];tx=[];subs=[];presents=[];targets=[]
        for n in range(1,7):
            t=(n-1)*1480;cid=2*n-1;address=0x100000+(n-1)*8
            commands.extend([dict(id=cid,submission=n,opcode=10,execute=t//148,complete_cycle=t//148+1),
                             dict(id=cid+1,submission=n,opcode=0,execute=t//148+2,complete_cycle=t//148+3)])
            targets.append([t,'target',[cid,0x100000,0x140000,1]])
            subs.append(dict(id=n,start_tick=t,end_tick=t+444))
            presents.extend([[t+592,'present_request',n],[t+740,'present_applied',[n,0x80000]]])
            tx.append(dict(port=2,writing=True,start_tick=t+148,address_halfword=address//2,requested_beats=1))
            writes.extend([[t+185+i*74,'physical_write',[address+4*i,n,15]] for i in range(2)])
        s=dict(axis_origin_tick=0,origin_tick=0,coverage_end_tick=8880,presents=presents,
               submissions=subs,transactions=tx,gpu=dict(image_events=targets,cache=[]),
               image_journal=dict(completed_target_budget=4,selected_presentations=[1,2,4,6]))
        data=dict(system=s,commands=commands,partial=False,
                  provenance=dict(report_inputs=dict(files={'initial-memory.bin':hashlib.sha256(initial).hexdigest()})))
        root=ROOT.parents[3]/'target/frame-reports/image-tests';root.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as folder:
            out=Path(folder)/'later';images=system_images.build(data,initial,writes,out,preview_mode='raw');data['images']=images
            system_images.validate(data)
            self.assertEqual(images['selected_submission_ids'],[1,2,4,6])
            self.assertEqual(set(images['commands']),{'1','2','3','4','7','8','11','12'})
            limited=copy.deepcopy(data);limited['system']['image_journal']['max_command_images']=7
            with self.assertRaisesRegex(ValueError,'image budget exceeded'):
                system_images.build(limited,initial,writes,Path(folder)/'over-budget')
            raw=(out/images['commands']['12']['color_file']).read_bytes()
            self.assertEqual(raw[:48],b''.join(n.to_bytes(4,'little')*2 for n in range(1,7)))
            frame=system_images.project(data,[6]);frame['system']=dict(frame['system'],frame_view=dict(owned_submission_ids=[6]))
            system_images.validate(frame)
            for bad in ([1,2,4,4],[1,2,4,7],[1,2,3,4,6]):
                broken=copy.deepcopy(data);broken['system']['image_journal']['selected_presentations']=bad
                with self.assertRaisesRegex(ValueError,'selected presentation'):system_images.selected_submissions(broken,4)
            missing=copy.deepcopy(data);missing['partial']=True;missing['commands'][-1]['complete_cycle']=None
            result=system_images.build(missing,initial,writes,Path(folder)/'open')
            self.assertNotIn('12',result['commands']);self.assertIn('remains open',result['unavailable_commands']['12'])

    def test_unbound_target_selection_is_an_explicit_same_submission_preimage(self):
        commands=[dict(id=1,submission=1,opcode=0,execute=1,complete_cycle=2),
                  dict(id=2,submission=1,opcode=10,execute=3,complete_cycle=4),
                  dict(id=3,submission=2,opcode=0,execute=6,complete_cycle=7),
                  dict(id=4,submission=2,opcode=10,execute=8,complete_cycle=9),
                  dict(id=5,submission=3,opcode=0,execute=11,complete_cycle=12)]
        data=dict(system=dict(axis_origin_tick=0),commands=commands)
        first=dict(command=2,color=0x100000,depth=0x140000,epoch=1,binding='actual committed SET_TARGET')
        second=dict(command=4,color=0x180000,depth=0x1c0000,epoch=3,binding='actual committed SET_TARGET')
        targets=[(4*148,first),(5*148,None),(9*148,second),(10*148,None)]
        preimage=system_images.select_target(data,targets,commands[0])
        self.assertEqual(preimage['selection'],'selected-output-preimage')
        self.assertIsNone(preimage['epoch'])
        self.assertEqual(preimage['command'],2)
        self.assertEqual(system_images.select_target(data,targets,commands[1])['selection'],'newly-bound-target')
        self.assertEqual(system_images.select_target(data,targets,commands[2])['command'],4)
        self.assertIsNone(system_images.select_target(data,targets,commands[4]))

    def test_masks_target_epochs_guards_and_missing_journal(self):
        root=ROOT.parents[3]/'target/frame-reports/image-tests';root.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as folder:
            root=Path(folder);initial=b'\x5a\xa5'*4194304;writes=[];transactions=[];commands=[]
            gpu=dict(image_events=[[0,'target',[1,0x100000,0x140000,1]],
                                   [1184,'target',[9,0x180000,0x1c0000,1]]],cache=[])
            s=dict(axis_origin_tick=0,origin_tick=0,coverage_end_tick=2516,presents=[],gpu=gpu,
                   submissions=[dict(id=1,start_tick=0,end_tick=2516)],transactions=transactions)
            commands.append(dict(id=1,submission=1,opcode=10,execute=0,complete_cycle=1))
            expected={base:bytearray(initial[:204800]) for base in (0x100000,0x180000)}
            snapshots={1:bytes(expected[0x100000])}
            for mask in range(16):
                base=0x100000 if mask<8 else 0x180000;tick=(mask+1)*148
                transactions.append(dict(port=2,writing=True,start_tick=tick,address_halfword=(base+128)//2,requested_beats=1))
                for half in range(2):
                    word=0x10203040+mask*65537+half;writes.append([tick+37+half*74,'physical_write',[base+128+half*4,word,mask]])
                    for byte in range(4):
                        if mask&(1<<byte):expected[base][128+half*4+byte]=(word>>(byte*8))&255
                cid=mask+2;commands.append(dict(id=cid,submission=1,opcode=10 if cid==9 else 0,
                    execute=mask+1,complete_cycle=mask+2))
                visible=0x180000 if cid>=9 else 0x100000;snapshots[cid]=bytes(expected[visible])
            data=dict(system=s,commands=commands,partial=False)
            images=system_images.build(data,initial,writes,root/'good',preview_mode='raw')
            for key,image in images['commands'].items():
                self.assertEqual((root/'good'/image['color_file']).read_bytes(),snapshots[int(key)])
                self.assertEqual((root/'good'/image['depth_file']).read_bytes(),initial[:204800])
            self.assertEqual(images['commands']['8']['target_epoch'],1)
            self.assertEqual(images['commands']['9']['target_epoch'],2)
            with self.assertRaisesRegex(ValueError,'incomplete physical'):
                system_images.build(data,initial,writes[:-1],root/'missing')
            bad=copy.deepcopy(writes);bad[0][2][0]+=4
            with self.assertRaisesRegex(ValueError,'physical address'):
                system_images.build(data,initial,bad,root/'wrong')
            opened=copy.deepcopy(data);opened['partial']=True;opened['commands'][-1]['complete_cycle']=None
            result=system_images.build(opened,initial,writes,root/'open',preview_mode='raw')
            self.assertNotIn('17',result['commands'])
            self.assertIn('remains open',result['unavailable_commands']['17'])
            cut=copy.deepcopy(data);cut['partial']=True;cut['system']['image_journal']={'truncated':True}
            result=system_images.build(cut,initial,writes,root/'cut',preview_mode='raw')
            self.assertNotIn('17',result['commands'])
            import hashlib
            data['images']=images
            data['provenance']=dict(report_inputs=dict(files={'initial-memory.bin':hashlib.sha256(initial).hexdigest()}))
            system_images.validate(data)
            bad=copy.deepcopy(data);bad['images']['commands']['1']['target_epoch']=None
            with self.assertRaisesRegex(ValueError,'target binding differs'):
                system_images.validate(bad)
            source=root/'source';(source/'images').mkdir(parents=True)
            import shutil
            for name in images['assets']:shutil.copyfile(root/'good'/name,source/'images'/name)
            system_images.copy_assets(data,source,root/'copied')
            name=next(iter(images['assets']));(source/'images'/name).write_bytes(b'wrong')
            with self.assertRaisesRegex(ValueError,'raw asset binding'):
                system_images.copy_assets(data,source,root/'refused')
            # GPU writes reach shared service after the real descriptor slot;
            # the path owner belongs to earlier master acceptance, not that PC.
            base=0x100000
            delayed=dict(partial=False,commands=[dict(id=1,submission=1,opcode=10,execute=0,complete_cycle=1),
                dict(id=2,submission=1,opcode=8,execute=1,complete_cycle=7)],system=dict(
                axis_origin_tick=0,origin_tick=0,coverage_end_tick=1184,presents=[],
                submissions=[dict(id=1,start_tick=0,end_tick=1184)],
                transactions=[dict(id=1,port=6,writing=True,start_tick=296,address_halfword=base//2,requested_beats=4)],
                gpu=dict(image_events=[[0,'target',[1,base,0x140000,1]],[148,'write_owner',[2,1]]],
                         cache=[],service_bindings=[[1,148]])))
            journal=[[333+i*74,'physical_write',[base+i*4,0x12345678,15]] for i in range(8)]
            result=system_images.build(delayed,initial,journal,root/'delayed',preview_mode='raw')
            raw=(root/'delayed'/result['commands']['2']['color_file']).read_bytes()
            self.assertEqual(raw[:32],bytes.fromhex('78563412')*8)
            self.assertEqual(raw[32:],initial[32:204800])
            partial=copy.deepcopy(delayed);partial['partial']=True
            partial['system']['transactions'][0]['end_tick']=1036
            with self.assertRaisesRegex(ValueError,'completed writer has missing'):
                system_images.build(partial,initial,journal[:-1],root/'missing-completed-prefix')


if __name__=='__main__':unittest.main()
