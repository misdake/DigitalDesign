import {GameComp, GameCompTemplate} from "./GameComp";
import {Game} from "./Game";
import {GameWire} from "./GameWire";
import {InputPinElement, OutputPinElement} from "../ui/component/PinElement";
import {EditorPin} from "./editor/EditorPin";
import {EditorComponent} from "./editor/EditorComponent";

export class Editor {
    private game: Game;
    public readonly pin: EditorPin;
    public readonly component: EditorComponent;

    //TODO 在这里统计原始对象和game对象之间的关系？

    constructor(game: Game) {
        //TODO 输入画布div
        this.game = game;

        this.pin = new EditorPin(game, this);
        this.component = new EditorComponent(game, this);

        // @ts-ignore
        window.deleteSelected = () => {
            if (this.selectedGameComp) {
                this.component.removeComponent(this.selectedGameComp);
                this.selectedGameComp = null;
            }
        };
    }

    private callbacks: (()=>void)[] = [];

    public registerUpdate(callback: () => void) {
        this.callbacks.push(callback);
    }

    public doUpdate() {
        this.callbacks.forEach(callback => callback()); //TODO 区分不同级别的修改
    }

    selectedGameComp: GameComp; //TODO move to Game?
    selectGameComp(gameComp: GameComp) {
        this.selectedGameComp = gameComp;
    }


}