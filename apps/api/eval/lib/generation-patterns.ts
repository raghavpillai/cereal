/** Pattern helpers for the generation fixtures. Notes may write a date as the
 * speakers did ("March 2", "March 2nd") or day first ("2 March", "2nd of
 * March"); both are the same answer. */
const SPOKEN=["","first","second","third","fourth","fifth","sixth","seventh","eighth","ninth","tenth","eleventh","twelfth","thirteenth","fourteenth","fifteenth","sixteenth","seventeenth","eighteenth","nineteenth","twentieth","twenty.first","twenty.second","twenty.third","twenty.fourth","twenty.fifth","twenty.sixth","twenty.seventh","twenty.eighth","twenty.ninth","thirtieth","thirty.first"]
export const day=(month:string,date:number)=>{
  const names=`(?:${month}|${month.slice(0,3)}\\.?${month==="September"?"|Sept\\.?":""})`
  // "9 December", "9th of December", and the first day of a range: "9–10 December", "9th and 10th December"
  // (the check normalises a dash between digits to a space before matching).
  return `${names} ${date}(?!\\d)|${names} ${SPOKEN[date]}|\\b${date}(?:st|nd|rd|th)?(?:\\s*(?:[–-]|and|to)?\\s*\\d{1,2}(?:st|nd|rd|th)?)? (?:of )?${names}`
}
