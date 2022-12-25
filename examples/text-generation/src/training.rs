use crate::{
    data::{Gpt2Tokenizer, TextGenerationBatcher, TextGenerationItem, Tokenizer},
    model::{TextClassificationModel, TextGenerationModelConfig},
};
use burn::data::dataset::transform::SamplerDataset;
use burn::{
    config::Config,
    data::{dataloader::DataLoaderBuilder, dataset::Dataset},
    module::Module,
    nn::transformer::TransformerEncoderConfig,
    optim::{Sgd, SgdConfig},
    tensor::backend::ADBackend,
    train::{
        metric::{AccuracyMetric, CUDAMetric, LossMetric},
        LearnerBuilder,
    },
};
use std::sync::Arc;

#[derive(Config)]
pub struct ExperimentConfig {
    transformer: TransformerEncoderConfig,
    optimizer: SgdConfig,
    #[config(default = 256)]
    max_seq_length: usize,
    #[config(default = 1)]
    batch_size: usize,
    #[config(default = 10)]
    num_epochs: usize,
}

pub fn train<B: ADBackend, D: Dataset<TextGenerationItem> + 'static>(
    device: B::Device,
    dataset_train: D,
    dataset_test: D,
    config: ExperimentConfig,
    artifact_dir: &str,
) {
    let dataset_train = Arc::new(SamplerDataset::new(Box::new(dataset_train), 10_000));
    let dataset_test = Arc::new(SamplerDataset::new(Box::new(dataset_test), 1_000));

    let tokenizer = Arc::new(Gpt2Tokenizer::default());
    let batcher_train = Arc::new(TextGenerationBatcher::new(
        tokenizer.clone(),
        config.max_seq_length,
    ));
    let batcher_test = Arc::new(TextGenerationBatcher::new(
        tokenizer.clone(),
        config.max_seq_length,
    ));

    let model = TextClassificationModel::<B>::new(&TextGenerationModelConfig::new(
        config.transformer.clone(),
        tokenizer.vocab_size(),
        tokenizer.pad_token(),
        config.max_seq_length,
    ));

    let dataloader_train = DataLoaderBuilder::new(batcher_train)
        .batch_size(config.batch_size)
        .num_workers(8)
        .build(dataset_train);

    let dataloader_test = DataLoaderBuilder::new(batcher_test)
        .batch_size(config.batch_size)
        .num_workers(8)
        .build(dataset_test);

    let optim = Sgd::new(&config.optimizer);

    let learner = LearnerBuilder::new(artifact_dir)
        .metric_train(CUDAMetric::new())
        .metric_valid(CUDAMetric::new())
        .metric_train(AccuracyMetric::new())
        .metric_valid(AccuracyMetric::new())
        .metric_train_plot(LossMetric::new())
        .metric_valid_plot(LossMetric::new())
        .with_file_checkpointer::<f32>(2)
        .devices(vec![device])
        .grads_accumulation(32)
        .num_epochs(config.num_epochs)
        .build(model, optim);

    let model_trained = learner.fit(dataloader_train, dataloader_test);

    config
        .save(&format!("{}/config.json", artifact_dir))
        .unwrap();
    model_trained
        .state()
        .convert::<f32>()
        .save(&format!("{}/model.json.gz", artifact_dir))
        .unwrap();
}