import {GameComp, GameCompTemplate} from "./GameComp";
import {Game} from "./Game";
import {GameWire, GameWireDummy} from "./GameWire";

export class Editor {
    private game: Game;

    constructor(game:Game) {
        //TODO 输入画布div
        this.game = game;


        // @ts-ignore
        window.deleteSelected = () => {
            if (this.selectedGameComp) {
                this.removeComponent(this.selectedGameComp);
                this.selectedGameComp = null;
            }
        };
    }

    private callbacks: (()=>void)[] = [];

    public registerUpdate(callback: () => void) {
        this.callbacks.push(callback);
    }

    private doUpdate() {
        this.callbacks.forEach(callback => callback()); //TODO 区分不同级别的修改
    }

    private nextCompId = 1;

    createComponent(template: GameCompTemplate, x: number, y: number): GameComp {
        let pack = {...template, x: x, y: y};
        let comp = new GameComp(this.nextCompId++, this.game.system, pack);
        this.game.components.push(comp);
        this.doUpdate(); //TODO 支持一次添加多个
        return comp;
    }

    moveComponent(gameComp: GameComp, x: number, y: number) {
        gameComp.x = x;
        gameComp.y = y;
        console.log("editor moveComponent", gameComp.name, x, y);
    }

    removeComponent(gameComp: GameComp): boolean {
        const index = this.game.components.indexOf(gameComp);
        if (index > -1) {

            //TODO 在这里删除相关的gameWire？

            this.game.components.splice(index, 1);

            this.doUpdate();

            return true;
        }
        return false;
    }


    createWireDummy() { //TODO 用什么参数
        //TODO 干掉之前的wireDummy[0]

        this.game.wireDummy[0] = new GameWireDummy();
        //TODO 刷新UI

        //返回这个wireDummy
    }

    createWire(gameWire: GameWire) {
        //TODO gameWire改为在这个方法里创建

        this.game.wires.push(gameWire);
        //TODO 刷新UI
    }

    removeWire(gameWire: GameWire) {
        const index = this.game.wires.indexOf(gameWire);
        if (index > -1) {
            this.game.wires.splice(index, 1);

            this.doUpdate(); //TODO 指明类型

            return true;
        }
        return false;
    }


    selectedGameComp: GameComp; //TODO move to Game?
    selectGameComp(gameComp: GameComp) {
        this.selectedGameComp = gameComp;
    }


}