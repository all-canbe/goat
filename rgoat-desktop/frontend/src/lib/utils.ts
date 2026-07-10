let _idCounter = 0;

export function generateId(): string {
  _idCounter += 1;
  return `msg_${Date.now()}_${_idCounter}`;
}
