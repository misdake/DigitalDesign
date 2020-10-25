import {Component, LogicRun, Wire} from "./Component";
import {ComponentInput, ComponentNot, ComponentOutput} from "./ComponentLib";

export class System {

    components: Component[];
    wires: Wire[];
    runners: LogicRun[];

    constructor() {
        let c1 = new ComponentInput(1);
        let c2 = new ComponentNot();
        let c3 = new ComponentOutput();

        let w1 = new Wire();
        w1.input = c1.output;
        w1.output = c2.input;
        let w2 = new Wire();
        w2.input = c2.output;
        w2.output = c3.input;


        this.components = [c1, c2, c3];
        this.wires = [w1, w2];
        this.runners = [c1, w1, c2, w2, c3];
    }

    run() {
        let c1 = this.components[0] as ComponentInput;
        c1.data = 1 - c1.data;

        this.runners.forEach(runner => runner.run());
    }

}