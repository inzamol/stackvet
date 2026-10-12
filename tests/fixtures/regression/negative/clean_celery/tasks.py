from celery import shared_task

# Clean Celery task: configured autoretry and time limit
@shared_task(autoretry_for=(Exception,), max_retries=3, time_limit=300, soft_time_limit=240)
def process_data(item_id):
    return item_id * 2

def trigger():
    # Asynchronous task dispatch using .delay()
    process_data.delay(10)
