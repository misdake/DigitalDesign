"""Passive activity accounting over an exact full-system time interval."""

def interval_activity(s, lo, hi):
    if lo is None or not s.get('states') or not s['origin_tick']<=lo<hi<=s['coverage_end_tick']:
        return dict(available=False,reason='Timing interval outside complete recorded state coverage')
    names=['cpu_runnable_cycles','gpu_busy_cycles','backend_busy_cycles',
           'cpu_runnable_gpu_busy_cycles','display_cpu_gpu_pending_cycles']
    totals=dict.fromkeys(names,0)
    covered=0
    for i,(t,state) in enumerate(s['states']):
        end=s['states'][i+1][0] if i+1<len(s['states']) else s['coverage_end_tick']
        ticks=max(0,min(end,hi)-max(t,lo));covered+=ticks
        runnable=not any(state[:5]);gpu=bool(state[5]);valid=state[8]
        flags=[runnable,gpu,bool(state[7]),runnable and gpu,bool(valid&1 and valid&6 and valid&112)]
        for name,flag in zip(names,flags):totals[name]+=ticks/148*flag
    if covered!=hi-lo:return dict(available=False,reason='Unobserved state prefix')
    client=[[0,0] for _ in range(7)];native=[0,0,0]
    for t,p,read,write in s.get('client_payload',[]):
        if lo<=t<hi:client[p][0]+=read;client[p][1]+=write
    for t,read,write,unmasked in s.get('native_model32',[]):
        if lo<=t<hi:native=[a+b for a,b in zip(native,(read,write,unmasked))]
    return dict(available=True,cycles=(hi-lo)/148,client64_payload_bytes=client,native32_bytes=native,**totals,
                scope='Exact timer window. CPU runnable excludes explicit holds/halt/fault; pipeline or memory stalls may remain. '
                      'Pending requests are interface valid signals, not physical pin occupancy.')
