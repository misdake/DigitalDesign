import {DependencyGraph} from "./DependencyGraph";
import {Component, ComponentBuiltin, ComponentGenerator, ComponentLogic, DummyWire, Pin, Wire} from "./Component";
import {ComponentTemplate} from "./ComponentTemplate";
import {error} from "./util";

export class System {

    componentTemplates: Map<string, ComponentTemplate> = new Map<string, ComponentTemplate>();
    componentGenerators: Map<string, ComponentGenerator> = new Map<string, ComponentGenerator>();

    private registerLibraryComponent(type: string, generator: ComponentGenerator) {
        this.componentGenerators.set(type, generator);
    }

    registerCustomComponent(template: ComponentTemplate) {
        this.componentTemplates.set(template.type, template);
        this.registerLibraryComponent(template.type, (name, _) => {
            return new Component(name, true, template, this.componentGenerators);
        });
    }

    registerBuiltinComponent(template: ComponentTemplate, logic: ComponentLogic) {
        this.componentTemplates.set(template.type, template);
        this.registerLibraryComponent(template.type, (name, componentLibrary) => {
            return new ComponentBuiltin(name, template, componentLibrary, logic);
        });
    }

    createComponent(name: string, type: string) {
        let generator = this.componentGenerators.get(type);
        if (!generator) error("Generator not found!");
        return generator(name, this.componentGenerators);
    }

    private mainComponent: Component;
    private runners: (Component | Pin | Wire)[];

    setMainComponent(component: Component) {
        this.mainComponent = component;
    }

    constructGraph() {
        let g = new DependencyGraph<Component | Pin, Wire>();

        function add(component: Component) {
            for (let child of Object.values(component.components)) {
                add(child);
            }

            if (component.isCustom) {
                //custom component => 添加pin作为vertex，不添加自己，添加内部所有wire
                for (let inputPin of Object.values(component.inputPins)) g.addVertex(inputPin);
                for (let outputPin of Object.values(component.outputPins)) g.addVertex(outputPin);

                for (let wire of component.wires) {
                    let from = wire.fromPin;
                    let to = wire.toPin;
                    g.addEdge(from, to, wire);
                }
            } else {
                //builtin component => 添加自己作为vertex，没有内部wire
                g.addVertex(component);
                for (let inputPin of Object.values(component.inputPins)) {
                    g.addVertex(inputPin);
                    g.addEdge(inputPin, component, new DummyWire());
                }
                for (let outputPin of Object.values(component.outputPins)) {
                    g.addVertex(outputPin);
                    g.addEdge(component, outputPin, new DummyWire());
                }
            }
        }

        add(this.mainComponent);

        //TODO 检查是否所有Wire的宽度都正常，所有的Wire都有两端

        let runners = g.calcOrder();
        this.runners = runners.filter(runner => runner.needRun);

        // console.log("order-----------------------");
        // for (let runner of this.runners) {
        //     console.log(runner.constructor.name, runner.name);
        // }
    }

    runLogic() {
        //TODO 清空所有Pin的数据
        for (let runner of this.runners) {
            runner.run();
        }
        // console.log("run-------------------------");
        // for (let input of this.mainComponent.inputPins.values()) {
        //     console.log(`input: ${input.name} => ${input.read()}`);
        // }
        // for (let output of this.mainComponent.outputPins.values()) {
        //     console.log(`output: ${output.name} => ${output.read()}`);
        // }
    }

}