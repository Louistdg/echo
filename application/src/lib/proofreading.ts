export interface ProofreadMatch {id:string;offset:number;length:number;original:string;message:string;suggestions:string[];kind:string;rule_id:string}
export interface ProofreadResult {ok:boolean;error?:string;request_id:string;source:string;matches:ProofreadMatch[];engine:string}
export function applySuggestion(current:string,checked:string,match:ProofreadMatch,replacement:string):string|null{
 if(current!==checked||!Number.isInteger(match.offset)||!Number.isInteger(match.length)||match.offset<0||match.length<0||current.slice(match.offset,match.offset+match.length)!==match.original)return null;
 return current.slice(0,match.offset)+replacement+current.slice(match.offset+match.length);
}
