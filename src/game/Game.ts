import {Editor} from "./Editor";
import {GameComp} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";
import {GameWire} from "./GameWire";

export class Game {
    readonly system: System;

    readonly components: GameComp[];
    readonly wires: GameWire[];

    readonly editor: Editor;

    constructor() {
        this.system = new System();
        registerBasicComponents(this.system);

        this.components = [];
        this.wires = [];

        this.editor = new Editor(this);
    }

}