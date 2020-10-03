import {Logic, Not, Reg} from "./Component";
import {OutputPin1} from "./Pin";

export class EmulatorSystem {

    constructor() {

    }

    regList: Reg[] = [];
    logicList: Logic[] = [];

    loadGraph() {
        this.regList = [];
        this.logicList = [];

        let mem = new Reg("mem", 1);
        this.regList.push(mem);
        let not = new Not();
        this.logicList.push(not);
        not.inputs["input"].connect(mem.outputs["currValue"]);
        mem.inputs["nextValue"].connect(not.outputs["output"]);
        mem.inputs["writeEnable"].connect(new OutputPin1(1));

        this.regList.forEach(i => i.initialize());
    }

    run() {
        for (let i = 0; i < 10; i++) {
            this.regList.forEach(i => i.startCycle());
            this.logicList.forEach(i => i.execute());
            this.regList.forEach(i => i.endCycle());

            console.log("after", i + 1, this.regList[0].value[0]);
        }
    }

}