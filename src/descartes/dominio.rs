use serde::{Deserialize, Serialize};

/// Estructura que representa un registro de descarte en el dominio de la panadería.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Descarte {
    /// Identificador único del documento en CouchDB (ej: descarte:01J9ZM1R...)
    pub _id: String,
    
    /// Tipo de documento, siempre será "descarte"
    pub tipo: String,
    
    /// Fecha de negocio en la que ocurre el descarte (YYYY-MM-DD)
    pub fecha: String,
    
    /// ID del lote de hornada al que pertenece el producto descartado
    pub lote_id: String,
    
    /// ID del producto descartado (ej: producto:pan-frances)
    pub producto_id: String,
    
    /// Cantidad de unidades descartadas
    pub unidades: i32,
    
    /// Motivo del descarte (ej: "danado", "vencido")
    pub motivo: String,
    
    /// Usuario (encargado) que registró el descarte
    pub usuario: String,
    
    /// Timestamp exacto de cuándo se creó localmente
    pub creado_en: String,
}

impl Descarte {
    /// Valida que los datos básicos del descarte sean lógicos (ej: unidades > 0)
    pub fn validar(&self) -> Result<(), String> {
        if self.unidades <= 0 {
            return Err("Las unidades a descartar deben ser mayores a cero.".to_string());
        }
        if self.motivo.trim().is_empty() {
            return Err("El motivo del descarte es obligatorio.".to_string());
        }
        Ok(())
    }
}